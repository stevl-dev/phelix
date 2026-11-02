//! Buffer kinds: how a feature's own buffer answers where the core would act
//! on an ordinary one. A feature declares its kind as a `static` and sets
//! `Document::kind` on the buffers it makes. A buffer with no kind takes the
//! core's own path.

use crate::{DocumentId, Editor, ViewId};

pub trait BufferKind: 'static {
    /// The buffer a language feature asks from for `view`'s primary selection,
    /// loaded if need be, with `view`'s selection at the same place in it. Or
    /// why there's none.
    fn source(&self, editor: &mut Editor, view: ViewId) -> Result<DocumentId, String>;
}
