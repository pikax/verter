# Semantic identity record ownership inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Interned<T>` owning handle and its record (value + content digest) | Every query and family key embedding a recipe, context, order domain, `RelateMemoKey` or `ResolveCallKey` identity | REQUIRED | Until the last owning handle drops; a record retains exactly the handles its value embeds | `src/semantic_query_memo/intern_table.rs` | always |
| `WeakInternTable` digest index (weak entries only) | Deduplication of the records above | REQUIRED | Process-wide index per identity kind; an entry is forgotten in the destructor of its record and backing capacity shrinks once the index drains | `src/semantic_query_memo/intern_table.rs` | always |
| Context, order-domain and recipe indexes (`CONTEXTS`, `ORDER_DOMAINS`, `RECIPES`) | `SemanticContext::intern`, `project_order_domain`, `IntersectionInputRef::{from_operands,from_steps}` | REQUIRED | As the index above; the production context is the one permanent record | `src/semantic_query/{semantic_context,intersection_input}.rs` | always |
| Family-key indexes (`RELATE_KEYS`, `RESOLVE_CALL_KEYS`) | `FamilyKey::Relate` / `FamilyKey::ResolveCall` | REQUIRED | As the index above; records live while a memo family or caller holds the handle | `src/semantic_query_memo/family_intern.rs` | always |
| Residency, length and capacity probes (`get`, `len`, `is_empty`, `capacity`, `context_records_resident`) | none; ownership and churn tests | OPTIONAL | Read on demand | `src/semantic_query_memo/intern_table.rs`, `src/semantic_query/semantic_context.rs` | `cfg(test)` |
