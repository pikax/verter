use std::{collections::HashMap, marker::PhantomData, num::NonZeroU32, sync::Arc};

/// A direct arena index, local to its originating snapshot and collection.
/// Never persist or transfer IDs between snapshots. `K` distinguishes collections
/// (including framework node families); IDs contain no authored position.
#[repr(transparent)]
pub struct Id<K>(NonZeroU32, PhantomData<fn() -> K>);
impl<K> Copy for Id<K> {}
impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> PartialEq for Id<K> {
    fn eq(&self, rhs: &Self) -> bool {
        self.0 == rhs.0
    }
}
impl<K> Eq for Id<K> {}
impl<K> std::hash::Hash for Id<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<K> std::fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.index().fmt(f)
    }
}
impl<K> Id<K> {
    pub(super) fn at(index: usize) -> Result<Self, super::StructureError> {
        let raw = u32::try_from(index)
            .ok()
            .and_then(|n| n.checked_add(1))
            .and_then(NonZeroU32::new)
            .ok_or(super::StructureError::Capacity)?;
        Ok(Self(raw, PhantomData))
    }
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

mod sealed {
    pub trait Sealed {}
}
/// Storage lifetime classes cannot be merged or extended by consumers.
/// Each class has a separate owner; dropping that owner releases its storage.
pub trait LifetimeClass: sealed::Sealed {}
macro_rules! classes {
    ($($name:ident),*) => {$(
        #[doc = concat!(stringify!($name), " storage lifetime marker.")]
        pub enum $name {}
        impl sealed::Sealed for $name {}
        impl LifetimeClass for $name {}
    )*};
}
classes!(Frontend, Semantic, CompilerScratch, TargetScratch, Emission);

/// Dense append-only storage. Hot records are `Copy`, so cannot own a String,
/// Vec or HashMap. Variable-size relations belong in a separate `FlatArena`.
pub struct Arena<K, T: Copy, L: LifetimeClass> {
    values: Vec<T>,
    marker: PhantomData<(fn() -> K, L)>,
}
impl<K, T: Copy, L: LifetimeClass> Default for Arena<K, T, L> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            marker: PhantomData,
        }
    }
}
impl<K, T: Copy, L: LifetimeClass> Arena<K, T, L> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, value: T) -> Result<Id<K>, super::StructureError> {
        let id = Id::at(self.values.len())?;
        self.values.push(value);
        Ok(id)
    }
    pub fn get(&self, id: Id<K>) -> Option<&T> {
        self.values.get(id.index())
    }
    pub fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A typed half-open range into one flat collection. Snapshot-local, like IDs.
pub struct FlatRange<K> {
    start: u32,
    end: u32,
    marker: PhantomData<fn() -> K>,
}
impl<K> Copy for FlatRange<K> {}
impl<K> Clone for FlatRange<K> {
    fn clone(&self) -> Self {
        *self
    }
}
/// Flat backing storage for attributes, operations, dependencies or relations.
pub struct FlatArena<K, T, L: LifetimeClass> {
    values: Vec<T>,
    marker: PhantomData<(fn() -> K, L)>,
}
impl<K, T, L: LifetimeClass> Default for FlatArena<K, T, L> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            marker: PhantomData,
        }
    }
}
impl<K, T, L: LifetimeClass> FlatArena<K, T, L> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn append(
        &mut self,
        values: impl IntoIterator<Item = T>,
    ) -> Result<FlatRange<K>, super::StructureError> {
        let start = self.values.len();
        for value in values {
            if self.values.len() == u32::MAX as usize {
                self.values.truncate(start);
                return Err(super::StructureError::Capacity);
            }
            self.values.push(value);
        }
        Ok(FlatRange {
            start: start as u32,
            end: self.values.len() as u32,
            marker: PhantomData,
        })
    }
    pub fn get(&self, range: FlatRange<K>) -> &[T] {
        &self.values[range.start as usize..range.end as usize]
    }
}

/// Interned normalized/decoded text. Raw authored text stays in source spans.
pub enum TextKey {}
pub type TextId = Id<TextKey>;
/// Only explicit intern requests allocate, once per distinct value. Lookup is
/// hashed; iteration order is never observable and IDs follow insertion order.
pub struct TextInterner<L: LifetimeClass> {
    values: Vec<Arc<str>>,
    lookup: HashMap<Arc<str>, TextId>,
    marker: PhantomData<L>,
}
impl<L: LifetimeClass> Default for TextInterner<L> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            lookup: HashMap::new(),
            marker: PhantomData,
        }
    }
}
impl<L: LifetimeClass> TextInterner<L> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn intern(&mut self, text: &str) -> Result<TextId, super::StructureError> {
        if let Some(&id) = self.lookup.get(text) {
            return Ok(id);
        }
        let id = Id::at(self.values.len())?;
        let text: Arc<str> = Arc::from(text);
        self.values.push(text.clone());
        self.lookup.insert(text, id);
        Ok(id)
    }
    pub fn resolve(&self, id: TextId) -> Option<&str> {
        self.values.get(id.index()).map(AsRef::as_ref)
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Rare side facts allocate only for present entries. Iteration follows dense
/// identity, independent of insertion order; hot classifications use `Arena`.
pub struct SparseFacts<K, T, L: LifetimeClass> {
    values: std::collections::BTreeMap<usize, T>,
    marker: PhantomData<(fn() -> K, L)>,
}
impl<K, T, L: LifetimeClass> Default for SparseFacts<K, T, L> {
    fn default() -> Self {
        Self {
            values: std::collections::BTreeMap::new(),
            marker: PhantomData,
        }
    }
}
impl<K, T, L: LifetimeClass> SparseFacts<K, T, L> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, id: Id<K>, value: T) -> Option<T> {
        self.values.insert(id.index(), value)
    }
    pub fn get(&self, id: Id<K>) -> Option<&T> {
        self.values.get(&id.index())
    }
    pub fn iter(&self) -> impl Iterator<Item = (Id<K>, &T)> {
        self.values
            .iter()
            .map(|(&index, value)| (Id::at(index).expect("stored index"), value))
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
