//! Buffer kinds: how a feature's own buffer answers where the core would act
//! on an ordinary one. A feature declares its kind as a `static` and sets
//! `Document::kind` on the buffers it makes. A buffer with no kind takes the
//! core's own path.

use helix_lsp::{lsp, OffsetEncoding};

use crate::{Document, DocumentId, Editor, ViewId};

pub trait BufferKind: 'static {
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
}
