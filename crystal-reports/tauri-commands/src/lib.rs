//! Drop-in Tauri command handlers for the Crystal Reports editor backend.
//!
//! With the default (empty) feature set this compiles as plain functions taking no Tauri
//! state beyond a shared reference — for compile-checking outside a Tauri app. Enable the
//! `tauri` feature in your app's dependency on this crate to turn each function into a real
//! `#[tauri::command]`.

pub mod commands;

#[cfg(feature = "tauri")]
pub mod glue;
