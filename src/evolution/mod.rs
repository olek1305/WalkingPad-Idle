//! Opcjonalna, niezależna kampania historii życia.

mod app;
pub(crate) mod campaign;
pub(crate) mod catalog;
pub(crate) mod persistence;
pub(crate) mod ui;

pub(crate) use app::run;
