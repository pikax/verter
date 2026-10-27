//! Pinned data-tree storage. Independently published owners share each raw
//! allocation's charge through the process registry; owned cells charge
//! themselves. No admission decision can refuse or revoke this storage.

#[cfg(any(test, feature = "test-support"))]
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use crate::allocation::{heap_block_bytes, AliasSet};
use crate::granular::GranularPin;
use crate::heap_size::{HeapMeter, HeapSize};
use crate::owned::ChargeCtx;
use crate::shared::{AliasCharges, CallerHold};
use crate::{SemanticRetentionAccount, StoreAccount};

/// Accounting context shared by every pinned artifact in the process.
#[derive(Clone, Debug)]
pub struct PinContext {
    account: StoreAccount,
    pub(crate) aliases: AliasCharges,
}

impl Default for PinContext {
    fn default() -> Self {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(context) = TEST_CONTEXT.with(|slot| slot.borrow().clone()) {
            return context;
        }
        static CONTEXT: OnceLock<PinContext> = OnceLock::new();
        CONTEXT
            .get_or_init(|| Self {
                account: StoreAccount::default(),
                aliases: AliasCharges::new(),
            })
            .clone()
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static TEST_CONTEXT: RefCell<Option<PinContext>> = const { RefCell::new(None) };
}

/// Restores the previous test context, including during unwinding.
#[cfg(any(test, feature = "test-support"))]
pub struct TestPinScope(
    Option<PinContext>,
    std::marker::PhantomData<std::rc::Rc<()>>,
);

#[cfg(any(test, feature = "test-support"))]
impl Drop for TestPinScope {
    fn drop(&mut self) {
        TEST_CONTEXT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

impl PinContext {
    /// Scope default pins on this thread to an isolated test account.
    /// Worker owners must explicitly carry and enter the same context.
    #[cfg(any(test, feature = "test-support"))]
    pub fn enter_for_test(&self) -> TestPinScope {
        TestPinScope(
            TEST_CONTEXT.with(|slot| slot.replace(Some(self.clone()))),
            std::marker::PhantomData,
        )
    }
    /// An isolated accounting world for deterministic ownership tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(account: Arc<SemanticRetentionAccount>) -> Self {
        Self {
            account: StoreAccount::new(account),
            aliases: AliasCharges::new(),
        }
    }

    pub fn account(&self) -> &Arc<SemanticRetentionAccount> {
        self.account.get()
    }

    /// Give back the shared-allocation registry's table capacity that
    /// released owners left behind. See [`AliasCharges::reclaim_table`].
    pub fn reclaim_shared_storage(&self) {
        self.aliases.reclaim_table();
    }
}

/// One owner's live backing and holds on shared allocations. Dropping the
/// owner releases its backing; shared storage stays charged to its other
/// published owners. The holds' own backing is included in the granular pin.
#[derive(Debug)]
pub struct PinnedHeap {
    _pins: Box<[CallerHold]>,
    backing: GranularPin,
}

impl PinnedHeap {
    pub fn measure(value: &impl HeapSize, inline: usize, context: &PinContext) -> Self {
        let mut handles = AliasSet::collecting();
        let mut meter = HeapMeter::pinning(&mut handles, context);
        let bytes = inline + value.heap_size(&mut meter);
        let pins = meter.finish_pins();
        handles
            .take_handles()
            .pin(ChargeCtx::new(context.account()));
        let pins = pins.into_boxed_slice();
        let mut backing = GranularPin::new(context.account(), 1024);
        backing.add(bytes + heap_block_bytes(std::mem::size_of_val(&*pins)));
        Self {
            _pins: pins,
            backing,
        }
    }

    pub fn used(&self) -> usize {
        self.backing.used()
    }
}

/// A charge embedded in a mutable assembly and sealed at publication.
/// Cloning an assembly creates new backing, so its charge is measured again
/// when that assembly is published. Shared child allocations stay shared.
#[derive(Default)]
pub struct StoragePin(OnceLock<PinnedHeap>);

/// Storage is accounting, not content: a record's `Debug` stays the same
/// whatever the account holds while it is printed.
impl std::fmt::Debug for StoragePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoragePin")
    }
}

impl Clone for StoragePin {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl StoragePin {
    pub fn invalidate(&mut self) {
        drop(self.0.take());
    }
    pub fn ensure(&self, value: &impl HeapSize, inline: usize, context: &PinContext) {
        self.0
            .get_or_init(|| PinnedHeap::measure(value, inline, context));
    }
}

impl HeapSize for StoragePin {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

/// Embedded storage for a content-free fact carrier. The only initializer
/// requires the compiler's transitive fact witness, so erased allocation
/// holds inside this pin cannot introduce a type-expression payload.
#[derive(Debug, Default, Clone)]
pub struct FactStoragePin(StoragePin);

impl FactStoragePin {
    pub fn ensure<T: HeapSize + verter_no_typeexpr::NoTypeExpr>(
        &self,
        value: &T,
        inline: usize,
        context: &PinContext,
    ) {
        self.0.ensure(value, inline, context);
    }
}

impl HeapSize for FactStoragePin {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

// The initializer above preserves the witness through the erased charge
// metadata, just as OwnedValue preserves its typed payload's witness.
impl verter_no_typeexpr::__private::NoTypeExprWitness for FactStoragePin {}

/// Default embedded-pin hook for a record whose storage is exposed through
/// this trait. The record's exhaustive HeapSize implementation still measures
/// all fields; only an Arc's publication calls this hook.
pub trait EmbeddedStorage: HeapSize {
    fn ensure_storage(&self, context: &PinContext);
}

pub fn pin_embedded<T: EmbeddedStorage>(value: &T, context: &PinContext) {
    value.ensure_storage(context);
}

/// A value with its own embedded pin, for a payload published behind an
/// `Arc` that readers clone out of their publication cell. The `Arc`'s
/// publication charges the whole allocation into this pin, so every reader
/// keeps the charge until the allocation itself is freed.
pub struct SelfPinned<T> {
    storage: StoragePin,
    value: T,
}

impl<T> SelfPinned<T> {
    pub fn new(value: T) -> Self {
        Self {
            storage: StoragePin::default(),
            value,
        }
    }
}

impl<T> std::ops::Deref for SelfPinned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for SelfPinned<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}

impl<T: HeapSize> HeapSize for SelfPinned<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        let Self { storage, value } = self;
        storage.heap_size(meter) + value.heap_size(meter)
    }

    fn pin_storage(&self, context: &PinContext) -> bool {
        self.storage.ensure(
            self,
            crate::allocation::arc_block_bytes(std::mem::size_of::<Self>()),
            context,
        );
        true
    }
}

/// A published write-once value with its pin, initialized in the same
/// transaction as the value. Failed setters drop their provisional pin.
/// Raw Arc children are internal to the cell. A child handed to readers must
/// own its storage pin (or use a sealed owned handle); cloning a raw Arc does
/// not clone the cell's adjacent charge.
pub struct PinnedOnce<T> {
    value: OnceLock<PinnedPayload<T>>,
}

struct PinnedPayload<T> {
    value: T,
    _pin: PinnedHeap,
}

impl<T> Default for PinnedOnce<T> {
    fn default() -> Self {
        Self {
            value: OnceLock::new(),
        }
    }
}

impl<T: HeapSize> PinnedOnce<T> {
    fn payload(value: T) -> PinnedPayload<T> {
        let pin = PinnedHeap::measure(&value, 0, &PinContext::default());
        PinnedPayload { value, _pin: pin }
    }

    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self) -> Option<&T> {
        self.value.get().map(|payload| &payload.value)
    }
    pub fn get_or_init(&self, build: impl FnOnce() -> T) -> &T {
        &self.value.get_or_init(|| Self::payload(build())).value
    }
    pub fn set(&self, value: T) -> Result<(), T> {
        self.value
            .set(Self::payload(value))
            .map_err(|payload| payload.value)
    }
}

impl<T: HeapSize> From<T> for PinnedOnce<T> {
    fn from(value: T) -> Self {
        Self {
            value: OnceLock::from(Self::payload(value)),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for PinnedOnce<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.get().map(|payload| &payload.value).fmt(f)
    }
}

impl<T: HeapSize + Clone> Clone for PinnedOnce<T> {
    fn clone(&self) -> Self {
        self.get()
            .map_or_else(Self::default, |value| Self::from(value.clone()))
    }
}

impl<T> HeapSize for PinnedOnce<T> {
    // The published payload carries its own pin; an outer Arc pays only
    // for the write-once cell's block.
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChargeClass, RetentionAdmission, RetentionLimits};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountedOwner {
        storage: StoragePin,
        checks: AtomicUsize,
        walks: AtomicUsize,
        text: String,
    }

    impl CountedOwner {
        fn new() -> Self {
            Self {
                storage: StoragePin::default(),
                checks: AtomicUsize::new(0),
                walks: AtomicUsize::new(0),
                text: String::with_capacity(1024 * 1024),
            }
        }
    }

    impl HeapSize for CountedOwner {
        fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
            let Self {
                storage,
                checks: _,
                walks,
                text,
            } = self;
            walks.fetch_add(1, Ordering::Relaxed);
            storage.heap_size(meter) + text.heap_size(meter)
        }

        fn pin_storage(&self, context: &PinContext) -> bool {
            self.checks.fetch_add(1, Ordering::Relaxed);
            self.storage.ensure(
                self,
                crate::allocation::arc_block_bytes(std::mem::size_of::<Self>()),
                context,
            );
            true
        }
    }

    #[test]
    fn pinned_walk_checks_embedded_owners_once_per_allocation() {
        let context = PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
        let owner = Arc::new(CountedOwner::new());
        let aliases = vec![Arc::clone(&owner); 64];
        let first = PinnedHeap::measure(&aliases, 0, &context);
        let checks = owner.checks.load(Ordering::Relaxed);
        println!(
            "embedded_pin_checks={checks} shared_aliases={}",
            aliases.len()
        );
        assert_eq!(
            checks, 1,
            "one allocation needs one ownership check per walk"
        );
        assert_eq!(owner.walks.load(Ordering::Relaxed), 1);
        let charged = context.account().snapshot().pinned_bytes;
        assert!(charged >= owner.text.capacity());
        let second = PinnedHeap::measure(&owner, 0, &context);
        assert_eq!(owner.checks.load(Ordering::Relaxed), 2);
        assert_eq!(owner.walks.load(Ordering::Relaxed), 1);
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        drop((aliases, first, second));
        assert!(context.account().snapshot().pinned_bytes >= owner.text.capacity());
        drop(owner);
        assert_eq!(context.account().snapshot().total_bytes(), 0);
    }

    #[test]
    fn concurrent_embedded_publications_measure_and_charge_the_payload_once() {
        let context = PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
        let owner = Arc::new(CountedOwner::new());
        let barrier = std::sync::Barrier::new(2);
        let pins = std::thread::scope(|scope| {
            let publish = || {
                barrier.wait();
                PinnedHeap::measure(&owner, 0, &context)
            };
            let first = scope.spawn(publish);
            let second = scope.spawn(publish);
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(owner.checks.load(Ordering::Relaxed), 2);
        assert_eq!(owner.walks.load(Ordering::Relaxed), 1);
        assert!(context.account().snapshot().pinned_bytes >= owner.text.capacity());
        drop(pins);
        assert!(context.account().snapshot().pinned_bytes >= owner.text.capacity());
        drop(owner);
        assert_eq!(context.account().snapshot().total_bytes(), 0);
    }

    #[derive(HeapSize)]
    struct Payload {
        raw: Arc<str>,
        eval: Arc<str>,
        names: Arc<Vec<String>>,
    }

    #[derive(HeapSize)]
    #[heap_size(pin = "crate::pinned_heap::pin_embedded")]
    struct Published {
        storage: StoragePin,
        names: Vec<String>,
    }

    impl EmbeddedStorage for Published {
        fn ensure_storage(&self, context: &PinContext) {
            self.storage.ensure(
                self,
                crate::allocation::arc_block_bytes(std::mem::size_of::<Self>()),
                context,
            );
        }
    }

    #[test]
    fn escaping_publication_reader_retains_its_embedded_charge() {
        let context = PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
        let _scope = context.enter_for_test();
        let cell = PinnedOnce::from(Arc::new(Published {
            storage: Default::default(),
            names: vec!["x".repeat(2 * 1024 * 1024)],
        }));
        let reader = cell.get().unwrap().clone();
        let charged = context.account().snapshot().pinned_bytes;
        assert!(charged >= reader.names[0].len());
        let second = PinnedHeap::measure(&reader, 0, &context);
        drop(second);
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        drop(cell);
        assert_eq!(
            context.account().snapshot().pinned_bytes,
            charged,
            "escaping reader keeps its charge after publication teardown"
        );
        drop(reader);
        assert_eq!(context.account().snapshot().pinned_bytes, 0);
    }

    #[test]
    fn pinned_trees_share_every_nested_allocation_in_both_release_orders() {
        for first_drops_first in [false, true] {
            let context =
                PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
            let source: Arc<str> = Arc::from("source".repeat(128 * 1024));
            let mut name = String::with_capacity(512 * 1024);
            name.push('x');
            let capacity = name.capacity();
            let payload = Arc::new(Payload {
                raw: source.clone(),
                eval: source,
                names: Arc::new(vec![name]),
            });
            let first = PinnedHeap::measure(&payload, 0, &context);
            let once = context.account().snapshot().pinned_bytes;
            assert!(
                once >= payload.raw.len() + capacity,
                "source bytes and spare string capacity are charged"
            );
            assert!(
                once < 2 * payload.raw.len() + capacity,
                "raw and eval alias one allocation"
            );
            let second = PinnedHeap::measure(&payload, 0, &context);
            assert_eq!(
                context.account().snapshot().pinned_bytes,
                once + second.backing.charged(),
                "another owner adds only its hold list"
            );
            let (early, late) = if first_drops_first {
                (first, second)
            } else {
                (second, first)
            };
            drop(early);
            assert_eq!(context.account().snapshot().pinned_bytes, once);
            drop(late);
            assert_eq!(context.account().snapshot().total_bytes(), 0);
        }
    }

    #[test]
    fn pressure_never_refuses_or_revokes_pinned_payloads() {
        let context = PinContext::new(SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: 0,
            max_entry_bytes: 0,
            ..RetentionLimits::defaults()
        }));
        let source: Arc<str> = Arc::from("oversized".repeat(128 * 1024));
        let pin = PinnedHeap::measure(&source, 0, &context);
        let charged = context.account().snapshot().pinned_bytes;
        assert!(charged >= source.len());
        assert!(matches!(
            context.account().reserve(ChargeClass::Retained, 1),
            RetentionAdmission::Refused(_)
        ));
        assert_eq!(context.account().snapshot().pinned_bytes, charged);
        assert_eq!(source.len(), 9 * 128 * 1024);
        drop(pin);
        assert_eq!(context.account().snapshot().total_bytes(), 0);
    }
}
