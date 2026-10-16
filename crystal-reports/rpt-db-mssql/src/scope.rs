//! The subreport data provider: one shared connection, each subreport scope fetched once and
//! memoized (identical scopes replay the cached rows instead of re-querying), failures noted
//! and surfaced as preview diagnostics while the render falls back to the scope's saved data —
//! the same semantics as the upstream drivers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use rpt_data::{Parameters, RowData, RowSource, ScopeData};
use rpt_model::Report;

use crate::conn::ConnectionConfig;
use crate::fetch::{fetch_report_table, TdsClient};
use crate::plan::plan_for_report;

/// A fetched scope's rows behind an `Arc`, exposed as a [`RowSource`] any number of times.
struct SharedRows(Arc<RowData>);

impl RowSource for SharedRows {
    fn columns(&self) -> &[rpt_data::Column] {
        RowSource::columns(self.0.as_ref())
    }
    fn rows(&self) -> Vec<rpt_data::Row> {
        RowSource::rows(self.0.as_ref())
    }
}

/// The live data provider handed to `rpt_render::RenderOptions::scope`. Owns the connection
/// (shared with the main-scope fetch by construction) and a fetch memo.
pub struct MssqlScope {
    config: ConnectionConfig,
    params: Parameters,
    client: Arc<Mutex<TdsClient>>,
    /// Fetch memo keyed by the scope's fetch identity (tables + resolved binds).
    cache: RefCell<HashMap<u64, Arc<RowData>>>,
    /// Non-fatal notes (fetch failures, mismatches) for the caller to surface as diagnostics.
    notes: RefCell<Vec<String>>,
}

impl MssqlScope {
    /// Wrap an open connection together with the render parameters. The connection is the
    /// SAME one the main scope was fetched through — one login per preview.
    pub fn new(config: ConnectionConfig, params: Parameters, client: Arc<Mutex<TdsClient>>) -> Self {
        Self {
            config,
            params,
            client,
            cache: RefCell::new(HashMap::new()),
            notes: RefCell::new(Vec::new()),
        }
    }

    /// Drain the accumulated notes (failures, mismatches) — call after the render.
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut *self.notes.borrow_mut())
    }

    fn scope_key(&self, report: &Report) -> u64 {
        let plans = plan_for_report(report, &self.params);
        let mut h = DefaultHasher::new();
        for p in &plans {
            p.describe().hash(&mut h);
        }
        h.finish()
    }
}

impl ScopeData for MssqlScope {
    fn rows_for(&self, report: &Report) -> Option<Box<dyn RowSource>> {
        let key = self.scope_key(report);
        if let Some(hit) = self.cache.borrow().get(&key) {
            return Some(Box::new(SharedRows(Arc::clone(hit))));
        }
        let mut client = self.client.lock().ok()?; // a panicked fetch poisons: fall back to saved data
        match fetch_report_table(&mut client, &self.config.label(), report, &self.params) {
            Ok(fetched) => {
                let arc = Arc::new(fetched.data);
                for note in fetched.notes {
                    self.notes.borrow_mut().push(note);
                }
                self.cache.borrow_mut().insert(key, Arc::clone(&arc));
                Some(Box::new(SharedRows(arc)))
            }
            Err(e) => {
                self.notes
                    .borrow_mut()
                    .push(format!("subreport datasource unavailable ({e}); using saved data"));
                None
            }
        }
    }
}
