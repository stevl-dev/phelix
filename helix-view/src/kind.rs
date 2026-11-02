//! Buffer kinds: how a feature's own buffer answers where the core would act
//! on an ordinary one. A feature declares its kind as a `static` and sets
//! `Document::kind` on the buffers it makes. A buffer with no kind takes the
//! core's own path.

use helix_core::movement::Direction;
use helix_lsp::{lsp, OffsetEncoding};

use crate::editor::{CloseError, GutterType};
use crate::gutter::GutterFn;
use crate::{Document, DocumentId, Editor, Theme, ViewId};

pub trait BufferKind: 'static {
    /// Names the kind, for the registry of its `helix-term` side.
    fn name(&self) -> &'static str;

    /// The buffer a language feature asks from for `view`'s primary selection,
    /// loaded if need be, with `view`'s selection at the same place in it. Or
    /// why there's none.
    fn source(&self, editor: &mut Editor, view: ViewId) -> Result<DocumentId, String>;

    /// The open buffer under `view`'s cursor that `source` would answer with,
    /// without loading it. For checks that run on every typed char, like
    /// trigger characters. Those read this buffer's text before the cursor,
    /// so it has to match the source's there.
    fn source_open<'e>(&self, editor: &'e Editor, view: ViewId) -> Option<&'e Document>;

    /// Whether the completion menu previews a selected item in the text.
    fn previews_completion(&self) -> bool {
        true
    }

    /// Readies an item that `view` accepts from the completion menu, asked of
    /// `source`'s buffer, to go into this one.
    fn accept_completion(
        &self,
        _editor: &mut Editor,
        _view: ViewId,
        _item: &mut lsp::CompletionItem,
        _encoding: OffsetEncoding,
    ) {
    }

    /// Closes the buffer `doc`. Refused with the buffers that hold it open,
    /// and their names.
    fn close(
        &self,
        editor: &mut Editor,
        doc: DocumentId,
        force: bool,
    ) -> Result<(), Vec<(DocumentId, String)>> {
        match editor.close_document(doc, force) {
            Err(CloseError::BufferModified(name)) => Err(vec![(doc, name)]),
            _ => Ok(()),
        }
    }

    /// Moves by `count` through the buffer, for `gn` and `gp`, or `false` to
    /// cycle through the buffers as usual.
    fn goto_buffer(&self, _editor: &mut Editor, _direction: Direction, _count: usize) -> bool {
        false
    }

    /// The `gutter` column in the buffer's views, or `None` for the core's.
    /// Asked for the line numbers, the diff signs, and the diagnostics.
    fn gutter<'d>(
        &self,
        _editor: &'d Editor,
        _doc: &'d Document,
        _theme: &Theme,
        _gutter: GutterType,
        _focused: bool,
    ) -> Option<GutterFn<'d>> {
        None
    }
}
