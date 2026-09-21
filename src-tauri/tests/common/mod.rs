//! Test doubles shared by the browser tests. Each integration test file is
//! its own crate, so not every file uses every helper.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::browser::cdp::{CdpError, Driver, Event};

type Handler =
    Box<dyn FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError> + Send>;

/// A browser that answers from a closure and remembers every call.
pub struct ScriptedDriver {
    pub calls: Vec<(String, serde_json::Value)>,
    handler: Handler,
    /// Events already waiting.
    pub events: VecDeque<Event>,
    /// Events that appear once a call to the named method has been made -
    /// how a test says "the load event follows Page.navigate".
    pub on_call_events: Vec<(String, Event)>,
    pub dialogs: Vec<String>,
}

impl ScriptedDriver {
    pub fn new(
        handler: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError>
            + Send
            + 'static,
    ) -> Self {
        ScriptedDriver {
            calls: vec![],
            handler: Box::new(handler),
            events: VecDeque::new(),
            on_call_events: vec![],
            dialogs: vec![],
        }
    }

    pub fn methods(&self) -> Vec<String> {
        self.calls.iter().map(|(m, _)| m.clone()).collect()
    }

    pub fn calls_to(&self, method: &str) -> Vec<serde_json::Value> {
        self.calls.iter().filter(|(m, _)| m == method).map(|(_, p)| p.clone()).collect()
    }
}

impl Driver for ScriptedDriver {
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        self.calls.push((method.to_string(), params.clone()));
        let mut fired = vec![];
        self.on_call_events.retain(|(m, ev)| {
            if m == method {
                fired.push(ev.clone());
                false
            } else {
                true
            }
        });
        self.events.extend(fired);
        (self.handler)(method, &params)
    }

    async fn wait_event(&mut self, method: &str, _limit: Duration) -> Result<Event, CdpError> {
        match self.events.iter().position(|e| e.method == method) {
            Some(i) => Ok(self.events.remove(i).expect("position was just found")),
            None => Err(CdpError::Timeout { what: method.to_string(), ms: 0 }),
        }
    }

    fn forget_events(&mut self) {
        self.events.clear();
    }

    fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
    }
}
