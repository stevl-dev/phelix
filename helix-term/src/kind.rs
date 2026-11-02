//! The `helix-term` side of buffer kinds: the answers that need the UI, which
//! `helix-view` can't see. A feature registers one for its kind's name at
//! startup.

use std::sync::RwLock;

use helix_view::{Document, DocumentId};

use crate::commands::WriteOptions;
use crate::compositor;

pub trait BufferKindUi: Sync + 'static {
    /// `:reload` on the buffer, and `:reload-all` with it open.
    fn reload(&self, cx: &mut compositor::Context, doc: DocumentId);

    /// `:write` on the buffer.
    fn write(
        &self,
        cx: &mut compositor::Context,
        doc: DocumentId,
        path: Option<&str>,
        options: WriteOptions,
    ) -> anyhow::Result<()>;
}

static REGISTRY: RwLock<Vec<(&'static str, &'static dyn BufferKindUi)>> = RwLock::new(Vec::new());

/// Registers `ui` for the kind named `name`. Again for the same name does
/// nothing, so every `Application` in the tests can register.
pub fn register(name: &'static str, ui: &'static dyn BufferKindUi) {
    let mut registry = REGISTRY.write().unwrap();
    if !registry.iter().any(|&(known, _)| known == name) {
        registry.push((name, ui));
    }
}

/// The UI part of `doc`'s kind, if it has one.
pub fn ui(doc: &Document) -> Option<&'static dyn BufferKindUi> {
    let name = doc.kind?.name();
    let registry = REGISTRY.read().unwrap();
    registry
        .iter()
        .find(|&&(known, _)| known == name)
        .map(|&(_, ui)| ui)
}
