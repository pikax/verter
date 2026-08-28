# Exact operative source-clause attachment — OPT0

Schema: 1. Node: `OPT0`. Clause count: 66. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1809-F93D1FD64CF5

- Kind: `context`; source: `compiler-proposal.md:1809-1809`; target: `node:OPT0`; text SHA-256: `f93d1fd64cf50e3f27ed8c9299c7e61cf3023428b3fc0f61cb5ed5bef6fa6b51`.

~~~~markdown
## `OPT0.md` — Compiler optimization engine rescope and maintainer ratification
~~~~

### SRC-COMP-L1811-39309A33AACD

- Kind: `requirement`; source: `compiler-proposal.md:1811-1811`; target: `node:OPT0`; text SHA-256: `39309a33aacdfed9773c9f8f55c6ce07a3f9c4d9c0cf2dcb13987c342d072cf0`.

~~~~markdown
**Status:** `RESCOPE_REQUIRED`; no implementation authority; no `OPT1+` block may be created from this proposal.
~~~~

### SRC-COMP-L1813-1D4133D972CC

- Kind: `context`; source: `compiler-proposal.md:1813-1813`; target: `node:OPT0`; text SHA-256: `1d4133d972cc6f81007a31166aed514f61438294440755e66b9dfaab508a63c2`.

~~~~markdown
**Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.
~~~~

### SRC-COMP-L1815-5D0D58141228

- Kind: `context`; source: `compiler-proposal.md:1815-1815`; target: `node:OPT0`; text SHA-256: `5d0d5814122845419a69ac4809a90a3f74d33c8d45d05ade8b64e57a84aabf24`.

~~~~markdown
**Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compilers.
~~~~

### SRC-COMP-L1817-C79BAF7E8EAE

- Kind: `context`; source: `compiler-proposal.md:1817-1817`; target: `node:OPT0`; text SHA-256: `c79baf7e8eaedf0f8f1086c676e8203ce4edaeeb9725384b305e3fd0bac2e1b2`.

~~~~markdown
**Suggested predecessors:** `CMP6`, `CPER3`.
~~~~

### SRC-COMP-L1819-2643F16A3C0E

- Kind: `requirement`; source: `compiler-proposal.md:1819-1819`; target: `node:OPT0`; text SHA-256: `2643f16a3c0ee51445a5111e75e8382cd2a69fad88f113693eea86ead4bfbc97`.

~~~~markdown
**Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:
~~~~

### SRC-COMP-L1821-B9E110B2BD43

- Kind: `context`; source: `compiler-proposal.md:1821-1821`; target: `node:OPT0`; text SHA-256: `b9e110b2bd4396a13875ea8ef2370bb9e38d62dcd64dc8ced6deb75c101ae5b9`.

~~~~markdown
- precise optimization goals and measurable benefit;
~~~~

### SRC-COMP-L1822-A8126630386F

- Kind: `requirement`; source: `compiler-proposal.md:1822-1822`; target: `node:OPT0`; text SHA-256: `a8126630386fcf65fe602db9b79c62eb1bc51a6bb85e5e05610969ce74bc7a07`.

~~~~markdown
- Verter-native analysis only (`verter_analysis`, `type_info`, resolver);
~~~~

### SRC-COMP-L1823-7B94C64730A0

- Kind: `context`; source: `compiler-proposal.md:1823-1823`; target: `node:OPT0`; text SHA-256: `7b94c64730a0969aa67c66ad24123281ebcd41a7bb05c61b90daaf0f58a4d44a`.

~~~~markdown
- internal analysis-depth strategy behind public `Optimized`;
~~~~

### SRC-COMP-L1824-75B65B46D0CD

- Kind: `context`; source: `compiler-proposal.md:1824-1824`; target: `node:OPT0`; text SHA-256: `75b65b46d0cd517e9971fe1256f34d9d8a705eebf22cdce167c11a105a932e81`.

~~~~markdown
- `OptimizationRequestBasis` versus `OptimizationObservationSet`;
~~~~

### SRC-COMP-L1825-D3557005EEBC

- Kind: `requirement`; source: `compiler-proposal.md:1825-1825`; target: `node:OPT0`; text SHA-256: `d3557005eebc54cc1b30d695f3912737eec313f7ad42b69670ced647b8a1b34b`.

~~~~markdown
- exact read-set validation, invalidation, cancellation and budgets;
~~~~

### SRC-COMP-L1826-519BB725643F

- Kind: `context`; source: `compiler-proposal.md:1826-1826`; target: `node:OPT0`; text SHA-256: `519bb725643fe6a12f21f0bfaff576193021f7e9927a8d691fb8f5b354521fdb`.

~~~~markdown
- evidence/provenance representation and whether a generalized proof system is justified;
~~~~

### SRC-COMP-L1827-65915DEAB937

- Kind: `context`; source: `compiler-proposal.md:1827-1827`; target: `node:OPT0`; text SHA-256: `65915deab9378180d21c3c2ef029a9ab6c64318fa0981fc132ac4c94b67b758b`.

~~~~markdown
- deterministic fallback to `Default`;
~~~~

### SRC-COMP-L1828-99826D178F45

- Kind: `context`; source: `compiler-proposal.md:1828-1828`; target: `node:OPT0`; text SHA-256: `99826d178f456a32585b0a232a6be5f85e747d4dd6cb163ac986694ddab437ae`.

~~~~markdown
- artifact identity and reproducibility;
~~~~

### SRC-COMP-L1829-32FFF6C39956

- Kind: `context`; source: `compiler-proposal.md:1829-1829`; target: `node:OPT0`; text SHA-256: `32fff6c39956549c8fd3aec5af7c5116319705839ef05b298966d5fdc99d98a4`.

~~~~markdown
- security, filesystem/package boundaries and RSS;
~~~~

### SRC-COMP-L1830-70D0F4F1B99D

- Kind: `context`; source: `compiler-proposal.md:1830-1830`; target: `node:OPT0`; text SHA-256: `70d0f4f1b99d21cae813606edf18a6b8faf75910408d5871a1a3bc718f82ebb4`.

~~~~markdown
- per-framework target admission;
~~~~

### SRC-COMP-L1831-C318FD090326

- Kind: `context`; source: `compiler-proposal.md:1831-1831`; target: `node:OPT0`; text SHA-256: `c318fd090326546f4d308e058fbd8a898c20114808e2315440cbe9a2b0c2e002`.

~~~~markdown
- independent benchmarks proving compile-cost versus runtime/code-size benefit.
~~~~

### SRC-COMP-L1833-B152B24F184D

- Kind: `acceptance`; source: `compiler-proposal.md:1833-1833`; target: `node:OPT0`; text SHA-256: `b152b24f184df2e03eaaf807ad77d18eb22610e11f6d6456bbbe2874ecce8013`.

~~~~markdown
**Acceptance:** only a newly ratified plan and DAG amendment can close `OPT0` and create successors.
~~~~

### SRC-COMP-L1835-E2458F3EFD28

- Kind: `forbidden`; source: `compiler-proposal.md:1835-1835`; target: `node:OPT0`; text SHA-256: `e2458f3efd286ea402d4d35a5f98fa03a93d53d98b7def029dcd3397a010d213`.

~~~~markdown
**Forbidden:** code, “temporary” project traversal, enabling `Optimized`, generic certificate/proof engines, or using ambient LSP facts.
~~~~

### SRC-COMP-L1837-16FC22FCDD87

- Kind: `deletion`; source: `compiler-proposal.md:1837-1837`; target: `node:OPT0`; text SHA-256: `16fc22fcdd872990b94e9c3502af796a3190dd16feeff7df23a009bee376b226`.

~~~~markdown
**Deletion/abort:** none; remain `RESCOPE_REQUIRED` until maintainer action.
~~~~

### SRC-COMP-L1839-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1839-1839`; target: `node:OPT0`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-COMP-L1841-82CBBFE2151B

- Kind: `context`; source: `compiler-proposal.md:1841-1841`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `82cbbfe2151b08433140c4017c13454466201128739640e06be137fd660ff582`.

~~~~markdown
## `VCB0.md` — Vue custom-block integration rescope
~~~~

### SRC-COMP-L1843-CCF519F7F9D5

- Kind: `requirement`; source: `compiler-proposal.md:1843-1843`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `ccf519f7f9d57c71bec7dda26c11c84313b9fc0738388ed59031ed6fecbef977`.

~~~~markdown
**Status:** `RESCOPE_REQUIRED`; no implementation authority.
~~~~

### SRC-COMP-L1845-EEB8892F8D99

- Kind: `context`; source: `compiler-proposal.md:1845-1845`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `eeb8892f8d99c9bd65e9b2b7ed103948a09482dd63efec759c162e556eb73e1b`.

~~~~markdown
**Intent:** reserve a post-Vue-V2 architecture decision for custom-block semantic/runtime integration.
~~~~

### SRC-COMP-L1847-FB8BE6BD4879

- Kind: `requirement`; source: `compiler-proposal.md:1847-1847`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `fb8be6bd487904acf2ff7b42e07dccb6e3a69c79ce90051a96e0d0df22cf1735`.

~~~~markdown
**Problem:** custom-block transformation requires role/language resolution, host routing, trust, ABI/lifetime, maps, artifacts, failure and publication contracts that should not be guessed before the compiler artifact/host boundaries are proven.
~~~~

### SRC-COMP-L1849-CCCD0C0CA33F

- Kind: `context`; source: `compiler-proposal.md:1849-1849`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `cccd0c0ca33faef4710b9df3608e1e787ba3e392652659bd5135daea9bded728`.

~~~~markdown
**Suggested predecessor:** `VCP7`.
~~~~

### SRC-COMP-L1851-965E4D11B37C

- Kind: `context`; source: `compiler-proposal.md:1851-1851`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `965e4d11b37c4334ca56be078065bee3451523a8143178d74c32cf9f239bcdef`.

~~~~markdown
**Already locked by CCA2/VCP6:**
~~~~

### SRC-COMP-L1853-A251F370D652

- Kind: `requirement`; source: `compiler-proposal.md:1853-1853`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `a251f370d6525a083245aa5e48998670bfb250d01f9f5d405c47974a37519882`.

~~~~markdown
- exact source-backed descriptor;
~~~~

### SRC-COMP-L1854-F38F5CB48282

- Kind: `context`; source: `compiler-proposal.md:1854-1854`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f38f5cb48282420677b24f3a147b21cf297c21ab1ebb8a2252594c6117efc5f9`.

~~~~markdown
- block tag/role and `lang` are separate dimensions;
~~~~

### SRC-COMP-L1855-4844BCFFB17B

- Kind: `context`; source: `compiler-proposal.md:1855-1855`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `4844bcffb17ba4f21750fbd56a50e3a977aa2b95702e3170b0e72aa649a1cd04`.

~~~~markdown
- unknown blocks are opaque;
~~~~

### SRC-COMP-L1856-14C86EC4E0EC

- Kind: `requirement`; source: `compiler-proposal.md:1856-1856`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `14c86ec4e0ec290509c451db5f27a0789714d4ba893738ac938c1e9c2f75642b`.

~~~~markdown
- attributes, `src`, order, regions and content availability are preserved;
~~~~

### SRC-COMP-L1857-5CAF8162F929

- Kind: `context`; source: `compiler-proposal.md:1857-1857`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `5caf8162f929ed3b49964b412361ffb19450c9e5fbc72d8afacffe1c4d7c0abb`.

~~~~markdown
- no implicit execution.
~~~~

### SRC-COMP-L1859-F1E7D1F40AFA

- Kind: `requirement`; source: `compiler-proposal.md:1859-1859`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f1e7d1f40afad899beb836eb264409332188220ceae38ca1b3a95a69fdbc8c64`.

~~~~markdown
**Future plan must address:** semantic provider API, runtime transformation API, host integration, isolation/trust, native/WASM ABI if any, map composition, cancellation, artifact publication and versioning.
~~~~

### SRC-COMP-L1861-419D41C01883

- Kind: `acceptance`; source: `compiler-proposal.md:1861-1861`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `419d41c01883b4c9e77fac76126165c1494bd5e736541ad5847a183ebf6876d3`.

~~~~markdown
**Acceptance:** only a separately ratified maintainer plan creates implementation successors.
~~~~

### SRC-COMP-L1863-DEA8EEE41DB7

- Kind: `forbidden`; source: `compiler-proposal.md:1863-1863`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `dea8eee41db774981fe18fdf8e13cd245700b91e3f4b7d80919de3de4eacad66`.

~~~~markdown
**Forbidden:** ad hoc `<docs>`/`<i18n>` special cases in the generic compiler, dynamic loading, or using `lang` as the semantic role.
~~~~

### SRC-COMP-L1865-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1865-1865`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-COMP-L1867-1A42E938FE30

- Kind: `requirement`; source: `compiler-proposal.md:1867-1867`; target: `contract:contracts/sizing.md`; text SHA-256: `1a42e938fe302b9421242f6f84d12808c8dab877a6fd65836b4cd16c99d57e30`.

~~~~markdown
# 11. Required amendments to successor expansion blocks
~~~~

### SRC-COMP-L2007-893CF6DB01B2

- Kind: `context`; source: `compiler-proposal.md:2007-2007`; target: `contract:contracts/sizing.md`; text SHA-256: `893cf6db01b2c1bc80fb9bd8f5ea5864f396fcd8a7cdf9ba19eccbfc4f30e637`.

~~~~markdown
## 12.2 BUDGET — ratified numeric constraints
~~~~

### SRC-COMP-L2009-ECC700FA7C81

- Kind: `context`; source: `compiler-proposal.md:2009-2009`; target: `contract:contracts/sizing.md`; text SHA-256: `ecc700fa7c81b38882bcb8c8c97c3bb627d40e3e175f1a2dc68b8d4dfc228d60`.

~~~~markdown
- full-source and region scan counts;
~~~~

### SRC-COMP-L2010-FE00D7061FAB

- Kind: `context`; source: `compiler-proposal.md:2010-2010`; target: `contract:contracts/sizing.md`; text SHA-256: `fe00d7061fab77e15e768e266147ae1c4f702feca8b985c887cc741df85fb80a`.

~~~~markdown
- source-sized, regional and graph visits;
~~~~

### SRC-COMP-L2011-57739594D503

- Kind: `context`; source: `compiler-proposal.md:2011-2011`; target: `contract:contracts/sizing.md`; text SHA-256: `57739594d503722be8e571650d5389bd30ca71c826b588cf928c724cf479a7a5`.

~~~~markdown
- expression parses and semantic fact production;
~~~~

### SRC-COMP-L2012-B9F3D8345605

- Kind: `context`; source: `compiler-proposal.md:2012-2012`; target: `contract:contracts/sizing.md`; text SHA-256: `b9f3d834560581884c3362554018f7d687d78475a57d259c057f61c54ceb6b8e`.

~~~~markdown
- node/region/overlay sizes;
~~~~

### SRC-COMP-L2013-06158F4F139A

- Kind: `context`; source: `compiler-proposal.md:2013-2013`; target: `contract:contracts/sizing.md`; text SHA-256: `06158f4f139a080bb13eab5c520d7de0419875268d28edf63c5864961f5f51a4`.

~~~~markdown
- allocations and bytes by lifetime class;
~~~~

### SRC-COMP-L2014-5634B1474A21

- Kind: `context`; source: `compiler-proposal.md:2014-2014`; target: `contract:contracts/sizing.md`; text SHA-256: `5634b1474a212b11219022b41f9d5daa136d87807f3ad8263a796c52adfe47af`.

~~~~markdown
- raw source copy bytes;
~~~~

### SRC-COMP-L2015-DF1FBD9E73D0

- Kind: `context`; source: `compiler-proposal.md:2015-2015`; target: `contract:contracts/sizing.md`; text SHA-256: `df1fbd9e73d01a54330622dd52baf6e5bc59345d9a39712602f2a407032304af`.

~~~~markdown
- target-plan/effect/edge counts;
~~~~

### SRC-COMP-L2016-DBCB0B3EAEA8

- Kind: `context`; source: `compiler-proposal.md:2016-2016`; target: `contract:contracts/sizing.md`; text SHA-256: `dbcb0b3eaea892e462b40ae23ace57d03c73aaf795aec71c44278fa9962281ce`.

~~~~markdown
- selector candidate and predicate work;
~~~~

### SRC-COMP-L2017-519D587E00AB

- Kind: `context`; source: `compiler-proposal.md:2017-2017`; target: `contract:contracts/sizing.md`; text SHA-256: `519d587e00ab379df00e8a228f746741af534769220091fa51578ba3d748e28b`.

~~~~markdown
- emitted/copy/map bytes and allocations;
~~~~

### SRC-COMP-L2018-2CF0FC8F9546

- Kind: `context`; source: `compiler-proposal.md:2018-2018`; target: `contract:contracts/sizing.md`; text SHA-256: `2cf0fc8f954602e6005cf1e4172ad83dd67ae04c9e9646bb0c53dd5c893891c5`.

~~~~markdown
- external style-stage file/dependency reads;
~~~~

### SRC-COMP-L2019-6623122E3336

- Kind: `context`; source: `compiler-proposal.md:2019-2019`; target: `contract:contracts/sizing.md`; text SHA-256: `6623122e3336a93dbefcf8b194c6c441c33d0fdf40921bc172b4711016542611`.

~~~~markdown
- cold/warm/batch latency;
~~~~

### SRC-COMP-L2020-3B055541EC50

- Kind: `context`; source: `compiler-proposal.md:2020-2020`; target: `contract:contracts/sizing.md`; text SHA-256: `3b055541ec504d5e4854ce9669949e5f81cac41e5bcf13c7d8e7d3a9d236cb3b`.

~~~~markdown
- cancellation waste;
~~~~

### SRC-COMP-L2021-28CE9459F363

- Kind: `context`; source: `compiler-proposal.md:2021-2021`; target: `contract:contracts/sizing.md`; text SHA-256: `28ce9459f3636f630d19fee419be9b4aa44a63a802516b9ef4e270c9dadbff46`.

~~~~markdown
- long-session RSS and idle CPU;
~~~~

### SRC-COMP-L2022-B0BA3D967082

- Kind: `context`; source: `compiler-proposal.md:2022-2022`; target: `contract:contracts/sizing.md`; text SHA-256: `b0ba3d96708277981e1a5184d3195b66a94450db709038cf3eb9855de75dd693`.

~~~~markdown
- direct/prepared/managed and multi-target reuse.
~~~~

### SRC-COMP-L2024-B7790E1C63F5

- Kind: `requirement`; source: `compiler-proposal.md:2024-2024`; target: `contract:contracts/sizing.md`; text SHA-256: `b7790e1c63f51ed0ea181abd8e11ec9754c6fd4e8d64726ad08643eb38ef3fc2`.

~~~~markdown
A budget may change only through an equivalent-work amendment and maintainer ratification.
~~~~

### SRC-COMP-L2073-29F12A7FF17C

- Kind: `context`; source: `compiler-proposal.md:2073-2073`; target: `contract:contracts/amendments.md`; text SHA-256: `29f12a7ff17c78aa0e0adb2b6380f4fc09a1d24e31635847ea2535cffee4f963`.

~~~~markdown
# 14. Final ratification recommendation
~~~~

### SRC-COMP-L2075-8AE046B0915C

- Kind: `context`; source: `compiler-proposal.md:2075-2075`; target: `contract:contracts/amendments.md`; text SHA-256: `8ae046b0915cca0161eb8746b9bb97849555a4a2200b7daff7c3fdccc5a08180`.

~~~~markdown
This compiler architecture should be merged with the following non-negotiable interpretation:
~~~~

### SRC-COMP-L2077-AE5E0B12CBEC

- Kind: `context`; source: `compiler-proposal.md:2077-2077`; target: `contract:contracts/amendments.md`; text SHA-256: `ae5e0b12cbec2a8fad0a4bfbed10ac50e22c8e96dccf4267f5c91e0fa019de33`.

~~~~markdown
- `Default`, not `Official`, is the supported baseline policy;
~~~~

### SRC-COMP-L2078-DFAAF58F23D9

- Kind: `context`; source: `compiler-proposal.md:2078-2078`; target: `contract:contracts/amendments.md`; text SHA-256: `dfaaf58f23d943004cec8c617e98a08dfbd0ac16589e42e55624d3475f8650af`.

~~~~markdown
- `Default` is correctness-first and may outperform or correct cheap upstream analysis where Verter can prove the result locally;
~~~~

### SRC-COMP-L2079-143D3796682C

- Kind: `context`; source: `compiler-proposal.md:2079-2079`; target: `contract:contracts/amendments.md`; text SHA-256: `143d3796682c27b34cc21f599df3be63b6da6ff54b7064c967bf02c1af8fde2a`.

~~~~markdown
- upstream compilers remain important differential references but do not define every Verter decision;
~~~~

### SRC-COMP-L2080-F90B3390B529

- Kind: `context`; source: `compiler-proposal.md:2080-2080`; target: `contract:contracts/amendments.md`; text SHA-256: `f90b3390b5292e6bcd6475d408f788a6a5bf95b17c14df4f2bc5455be2980272`.

~~~~markdown
- `Optimized` is named but not implemented;
~~~~

### SRC-COMP-L2081-648D856CF494

- Kind: `context`; source: `compiler-proposal.md:2081-2081`; target: `contract:contracts/amendments.md`; text SHA-256: `648d856cf494dec3b6bf1722001be2669466ccb82c5775b2c0bcab2932bef26b`.

~~~~markdown
- one semantic authority exists per framework epoch, not globally;
~~~~

### SRC-COMP-L2082-3EF329EDED84

- Kind: `context`; source: `compiler-proposal.md:2082-2082`; target: `contract:contracts/amendments.md`; text SHA-256: `3ef329eded8417074e02ed6134cff2dfc029127e5f9b9db20899e62697419e8b`.

~~~~markdown
- dense IDs, side tables, region ownership and optional materialization are the data-layout foundation;
~~~~

### SRC-COMP-L2083-28956834E860

- Kind: `requirement`; source: `compiler-proposal.md:2083-2083`; target: `contract:contracts/amendments.md`; text SHA-256: `28956834e8606e5e1e7f6161e7e4604b4aefff617a76885524f8e22fb88d9e0a`.

~~~~markdown
- both Vue and Svelte receive framework-owned selector-query capabilities, but only Svelte’s is a default compiler prerequisite;
~~~~

### SRC-COMP-L2084-1E596B68254F

- Kind: `context`; source: `compiler-proposal.md:2084-2084`; target: `contract:contracts/amendments.md`; text SHA-256: `1e596b68254f8efdfc2f962267d147b215650f968a0b311e44e380290c12b4c5`.

~~~~markdown
- CSS syntax/neutral facts stay in J and preprocessors stay external;
~~~~

### SRC-COMP-L2085-DCCB4727A9D8

- Kind: `requirement`; source: `compiler-proposal.md:2085-2085`; target: `contract:contracts/amendments.md`; text SHA-256: `dccb4727a9d80a12c4c33dd29ada02fc9324136a14b3c9e472b7cedeacc4ce16`.

~~~~markdown
- the immediate Rev11 bridge remains only `CCA0`–`CCA2`;
~~~~

### SRC-COMP-L2086-66B4895099FB

- Kind: `context`; source: `compiler-proposal.md:2086-2086`; target: `contract:contracts/amendments.md`; text SHA-256: `66b4895099fb55c5246889fe27c9b36d2b89dfd6f0d16281243807379e381d52`.

~~~~markdown
- the full compiler program remains an independent successor train;
~~~~

### SRC-COMP-L2087-F3E025D63EE5

- Kind: `context`; source: `compiler-proposal.md:2087-2087`; target: `contract:contracts/amendments.md`; text SHA-256: `f3e025d63ee5af745659337f03e95ca55fa4f0ebae56420c51722b31b6c19077`.

~~~~markdown
- implementation evidence from Vue Default and then Svelte Default is the next authority for further architecture changes.
~~~~
