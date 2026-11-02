//! The `helix-term` side of buffer kinds: the answers that need the UI, which
//! `helix-view` can't see. A feature registers one for its kind's name at
//! startup.

use std::collections::HashMap;
use std::sync::RwLock;

use arc_swap::access::Constant;
use helix_core::syntax::OverlayHighlights;
use helix_view::document::Mode;
use helix_view::{view, Document, DocumentId, Editor, Theme, View};

use crate::commands::WriteOptions;
use crate::compositor;
use crate::keymap::{KeyTrie, Keymaps};
use crate::ui::text_decorations::DecorationManager;

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

    /// The keymap while the buffer has focus, built from the user's, or
    /// `None` to keep the user's.
    fn keymap(&self, _user: &HashMap<Mode, KeyTrie>) -> Option<HashMap<Mode, KeyTrie>> {
        None
    }

    /// Draws over the buffer in `view`: decorations, and overlays after its
    /// syntax highlights.
    fn decorate<'a>(
        &self,
        _editor: &'a Editor,
        _doc: &'a Document,
        _view: &View,
        _theme: &Theme,
        _decorations: &mut DecorationManager<'a>,
        _overlays: &mut Vec<OverlayHighlights>,
    ) {
    }
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

/// Swaps a kind's keymap in while its buffer has focus. A whole `Keymaps`
/// swaps, so pending keys and sticky modes stay with the keymap they started
/// in.
#[derive(Default)]
pub struct KindKeys {
    /// The kind whose keymap is in `EditorView::keymaps`, and the user's.
    layer: Option<(&'static str, Keymaps)>,
}

impl KindKeys {
    pub fn swap(&mut self, keymaps: &mut Keymaps, editor: &Editor) {
        let doc = &editor.documents[&view!(editor).doc];
        let kind = doc.kind.map(|kind| kind.name());
        if kind.is_some() && self.layer.as_ref().map(|(name, _)| *name) == kind {
            return;
        }
        // WHY: from one kind's buffer straight to another's, the next kind's
        // keymap is built from the user's, not the first kind's.
        if let Some((_, user)) = self.layer.take() {
            *keymaps = user;
        }
        let (Some(kind), Some(ui)) = (kind, ui(doc)) else {
            return;
        };
        let Some(map) = ui.keymap(&keymaps.map()) else {
            return;
        };
        let layer = Keymaps::new(Box::new(Constant(map)));
        self.layer = Some((kind, std::mem::replace(keymaps, layer)));
    }
}
