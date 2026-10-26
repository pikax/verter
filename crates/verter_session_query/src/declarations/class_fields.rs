//! The table of class fields read through a synthetic value declaration, keyed by span
//! starts. Filled by the header walk.

use rustc_hash::{FxHashMap, FxHashSet};

/// Where a class field's synthetic value is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassFieldValueSource {
    /// The initializer's type derives from a call: the indexed program
    /// expression at the initializer.
    Call,
    /// The initializer reads `this`, or holds a callback that may: a served
    /// position of its own, whose frame reads the receiver.
    Initializer,
}

/// The classification of every field of the classes one header walk
/// indexed, by initializer offset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassFieldValues {
    /// The classes classified here, by span start.
    classes: FxHashSet<u32>,
    /// The fields read through a synthetic value, by initializer offset.
    fields: FxHashMap<u32, ClassFieldValueSource>,
}

impl ClassFieldValues {
    /// Record that the class starting at `class_start` was classified here.
    pub fn record_class(&mut self, class_start: u32) {
        self.classes.insert(class_start);
    }

    /// Record the classification of the field whose initializer starts at
    /// `value_start`.
    pub fn record_field(&mut self, value_start: u32, kind: ClassFieldValueSource) {
        self.fields.insert(value_start, kind);
    }

    /// Whether the class starting at `class_start` was classified here.
    #[must_use]
    pub fn classified_class(&self, class_start: u32) -> bool {
        self.classes.contains(&class_start)
    }

    /// The recorded classification of the field whose initializer starts at
    /// `value_start`.
    #[must_use]
    pub fn recorded_field(&self, value_start: u32) -> Option<ClassFieldValueSource> {
        self.fields.get(&value_start).copied()
    }
}
