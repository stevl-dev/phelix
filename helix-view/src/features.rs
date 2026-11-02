//! Feature state on the editor, kept by type, so the editor names no feature.
//! A feature reaches its own through `Editor::features`, a field rather than a
//! method on `Editor`, so a borrow of the state and one of `editor.documents`
//! can sit side by side.

use std::any::{Any, TypeId};
use std::collections::HashMap;

#[derive(Default)]
pub struct Features {
    state: HashMap<TypeId, Box<dyn Any>>,
}

impl Features {
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.state.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn get_or_default<T: Default + 'static>(&mut self) -> &mut T {
        self.state
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::<T>::default())
            .downcast_mut()
            .expect("kept by its type")
    }
}
