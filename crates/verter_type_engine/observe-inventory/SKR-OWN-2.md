# Semantic identity record ownership inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Interned<T>` owning handle and its record (value + content digest) | Every query and family key embedding a recipe, context, order domain, `RelateMemoKey` or `ResolveCallKey` identity | REQUIRED | Until the last owning handle drops; a record retains exactly the handles its value embeds, and a destroyed record reclaims its released same-kind children (`InternDomain::take_children`) iteratively | `src/semantic_query_memo/intern_table.rs` | always |
| `WeakInternTable` digest index (weak entries only) | Deduplication of the records above | REQUIRED | Process-wide index per identity kind; an entry is forgotten in the destructor of its record, and both a surviving collision bucket's and the index's backing capacity shrink once they drain | `src/semantic_query_memo/intern_table.rs` | always |
| Context, order-domain and recipe indexes (`InternDomain::index()` for `SemanticContext`, `OrderDomainKey` and `IntersectionRecipe`) | `SemanticContext::intern`, `project_order_domain`, `IntersectionInputRef::{from_operands,from_steps}` | REQUIRED | As the index above; the production context is the one permanent record | `src/semantic_query/{semantic_context,intersection_input}.rs` | always |
| Family-key indexes (`InternDomain::index()` for `RelateMemoKey` and `ResolveCallKey`) | `FamilyKey::Relate` / `FamilyKey::ResolveCall` | REQUIRED | As the index above; records live while a memo family or caller holds the handle | `src/semantic_query_memo/family_intern.rs` | always |
