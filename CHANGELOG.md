# Changelog

## [0.4.4](https://github.com/PackWeave/weave/compare/v0.4.3...v0.4.4) (2026-03-24)


### Features

* **cli:** add --dry-run flag to install, remove, sync, and use ([#166](https://github.com/PackWeave/weave/issues/166)) ([f4dec45](https://github.com/PackWeave/weave/commit/f4dec452f4878d47418c49ae6c9dbd48acbcf955))


### Bug Fixes

* **adapters:** switch Codex adapter to toml_edit for comment preservation ([#227](https://github.com/PackWeave/weave/issues/227)) ([c7ac69d](https://github.com/PackWeave/weave/commit/c7ac69dc04ecff4cd7b6d197283477c69768c5e5))
* **cli:** make --dry-run truly non-mutating ([#229](https://github.com/PackWeave/weave/issues/229)) ([fd8a991](https://github.com/PackWeave/weave/commit/fd8a991f367c0dd426ac4a491ecd87d4887776db))

## [0.4.3](https://github.com/PackWeave/weave/compare/v0.4.2...v0.4.3) (2026-03-23)


### Bug Fixes

* **release:** make workflow_dispatch tag optional and pass explicit tag_name ([#211](https://github.com/PackWeave/weave/issues/211)) ([77c5783](https://github.com/PackWeave/weave/commit/77c5783b9beb39322e60442f3ce49a3e0a4d1d1d))
* **release:** use PAT for Release Please to trigger release pipeline ([#209](https://github.com/PackWeave/weave/issues/209)) ([4a34f3f](https://github.com/PackWeave/weave/commit/4a34f3f18829fdf689a9dc9259373f6a405e4376))

## [0.4.2](https://github.com/PackWeave/weave/compare/v0.4.1...v0.4.2) (2026-03-23)


### Features

* **cli:** implement weave auth for registry authentication ([#147](https://github.com/PackWeave/weave/issues/147)) ([012e40c](https://github.com/PackWeave/weave/commit/012e40c2475b91588dd5183785af827b5fe56d68))
* **cli:** implement weave auth for registry authentication ([#147](https://github.com/PackWeave/weave/issues/147)) ([8b6fa4e](https://github.com/PackWeave/weave/commit/8b6fa4e451ad2977519db14235cce4aa8afe1b4d))
* **cli:** implement weave publish for registry pack publishing ([#146](https://github.com/PackWeave/weave/issues/146)) ([#207](https://github.com/PackWeave/weave/issues/207)) ([58c744a](https://github.com/PackWeave/weave/commit/58c744a6fbbd1bf62ff13e503df4dbbf7180d3eb))
* **hooks:** additive per-event merge with ownership tracking ([38ea033](https://github.com/PackWeave/weave/commit/38ea033323d22f792b41dfb9c05bc4478ac42ea0))
* **hooks:** additive per-event merge with ownership tracking ([#145](https://github.com/PackWeave/weave/issues/145)) ([1511be4](https://github.com/PackWeave/weave/commit/1511be44a4a257a3887adce9dcd6c57ffb635e7c))
* **release:** add Release Please for automated changelog and release PRs ([94ddfae](https://github.com/PackWeave/weave/commit/94ddfae8a113447a5bf364e8942e0bdeef4c06f9))
* **release:** add Release Please for automated changelog and release PRs ([6ad5752](https://github.com/PackWeave/weave/commit/6ad575238d82007018f1858a94e493c98ca14456)), closes [#43](https://github.com/PackWeave/weave/issues/43)
* validate MCP server headers for plaintext secrets ([46646bf](https://github.com/PackWeave/weave/commit/46646bf1b5a29f1bd87cb6f98afdda054774eb86))
* validate MCP server headers for plaintext secrets ([0cefc4f](https://github.com/PackWeave/weave/commit/0cefc4f893f9a89676c31aa51175b631c34e606f)), closes [#141](https://github.com/PackWeave/weave/issues/141)


### Bug Fixes

* add context-specific hints to NotInstalled error ([d3ea4ae](https://github.com/PackWeave/weave/commit/d3ea4aee261fe5e92d91d1c4303bf8862f11fe6c))
* address review findings for list_cached source info ([90c349a](https://github.com/PackWeave/weave/commit/90c349a12127e0bf9cf04d2787a6ab3fc434ae8a))
* address review findings for use_profile decoupling ([936d749](https://github.com/PackWeave/weave/commit/936d749b4deb8bcd7293e9b9b94c3772023f6106))
* address review findings for WeaveError migration ([bb57ee4](https://github.com/PackWeave/weave/commit/bb57ee48a2564a37995dc29d75c42022b55bef1a))
* collapse nested if blocks per clippy collapsible_if ([83ab84b](https://github.com/PackWeave/weave/commit/83ab84b9b3fdfa2f4aea79b7b4d1a104b6e8fab5))
* final audit fixes — test assertion, doc accuracy, code comment ([e476412](https://github.com/PackWeave/weave/commit/e476412ee992b9e4691f0ddaed62840cc4121949))
* **hooks:** address review findings for additive hook merge ([6dac7f7](https://github.com/PackWeave/weave/commit/6dac7f760327d462bd49cbdce115019ebe2220da))
* **hooks:** strip hooks from settings fragment and add mixed-state test ([4e09300](https://github.com/PackWeave/weave/commit/4e09300a46dba52ee795f871cf968b52112ceae6))
* include source indicator in Store::list_cached() return type ([671d581](https://github.com/PackWeave/weave/commit/671d58154915bc2c5780e2cbffb52978f85e3214))
* include source indicator in Store::list_cached() return type ([0d5446e](https://github.com/PackWeave/weave/commit/0d5446ea3a2672b4c537f906a5e3bfceba1281c8)), closes [#134](https://github.com/PackWeave/weave/issues/134)
* move .claude/worktrees/ under its own comment in .gitignore ([#194](https://github.com/PackWeave/weave/issues/194)) ([51c98bb](https://github.com/PackWeave/weave/commit/51c98bbc332c18b4bde6992b80fbf2f6c4560dcf))
* **release:** address review findings for Release Please setup ([6883c64](https://github.com/PackWeave/weave/commit/6883c64c66675d4d1b9914682d622176fa6f2280))
* remove duplicate .gitignore entry for .claude/worktrees/ ([f5c5f33](https://github.com/PackWeave/weave/commit/f5c5f33c85a916066484cb569124dd3ffa8a2325))
* remove duplicate .gitignore entry for .claude/worktrees/ ([115d414](https://github.com/PackWeave/weave/commit/115d4145dc0547eece8e41f6c4b2253ada867327))
* remove duplicate .gitignore entry for .claude/worktrees/ ([f6378d7](https://github.com/PackWeave/weave/commit/f6378d73c9aa1324836f7a6708fb10ca1a9aca55))
* remove duplicate .gitignore entry for .claude/worktrees/ ([93c4c43](https://github.com/PackWeave/weave/commit/93c4c435d72747d2ed5304263de57863c6c76b60))
* remove duplicate .gitignore entry for .claude/worktrees/ ([2d1e3ce](https://github.com/PackWeave/weave/commit/2d1e3ce40f55d29c518e6c6f67a5c79373d0051f))
* remove duplicate .gitignore entry for .claude/worktrees/ ([d5168d3](https://github.com/PackWeave/weave/commit/d5168d308ebfd6ee639ae8b6b99bf6ef12be3b40))
* remove duplicate .gitignore entry for .claude/worktrees/ ([39ae930](https://github.com/PackWeave/weave/commit/39ae93053c24f22964dea7f7e0f554924ee82c82))
* remove duplicate .gitignore entry for .claude/worktrees/ ([88b309d](https://github.com/PackWeave/weave/commit/88b309d1a1d9d4b9af47f2a9693ca4508161ce63))
* replace anyhow::Error with WeaveError in core orchestration modules ([12e8286](https://github.com/PackWeave/weave/commit/12e828620f221070e5841edc6a5ee79aed6c3907))
* replace anyhow::Error with WeaveError in core orchestration modules ([11045f1](https://github.com/PackWeave/weave/commit/11045f125d1545ab4e2a31f045125593983eaf88))
* resolve merge conflict — combine WeaveError types with Registry trait decoupling ([885641c](https://github.com/PackWeave/weave/commit/885641c4ce457b864031193a71bc9ff26b288bfa))
* resolve merge conflict with normalize_path tests from main ([0d3f94e](https://github.com/PackWeave/weave/commit/0d3f94e67621926e47dee5fc762f535e28177063))
* restore PackSource guard in use_profile and add tests ([479e526](https://github.com/PackWeave/weave/commit/479e526ed59d8ab275444b4534ec1c906c7b0fa7))
* **security:** address code review — host allowlist, source display, test coverage ([7151f1a](https://github.com/PackWeave/weave/commit/7151f1a9c1ccbb13b26852c7a863b794f5c904ae))
* **security:** harden auth after audit — 9 code fixes, 13 new tests ([7b1fa8d](https://github.com/PackWeave/weave/commit/7b1fa8d3947742b30b63ae569095773565af9760))
* **security:** harden auth token handling against credential theft and injection ([28fd7b8](https://github.com/PackWeave/weave/commit/28fd7b849d4fb61bc65e69f3065490ba246dc1bf))
* **security:** harden header secret detection per review findings ([e6aa680](https://github.com/PackWeave/weave/commit/e6aa6801bc9fb304495760ae2b82ef18b60c1f61))
* **security:** use unpredictable temp files and surface auth warnings ([#203](https://github.com/PackWeave/weave/issues/203), [#204](https://github.com/PackWeave/weave/issues/204)) ([#205](https://github.com/PackWeave/weave/issues/205)) ([b916f6c](https://github.com/PackWeave/weave/commit/b916f6cd3c9af92f507f6e69b399e8460255c2aa))
* **store:** address review findings for path normalization ([3230ba2](https://github.com/PackWeave/weave/commit/3230ba21d9342185ad450a9845d9b3dcb7f399cc))
* **store:** handle cross-platform path separators in normalization ([be5f53c](https://github.com/PackWeave/weave/commit/be5f53ca272c5e9a8e57efd27b2b0132e7f51686))
* **store:** normalize local paths before hashing in cache key ([8ed2ced](https://github.com/PackWeave/weave/commit/8ed2ced04244e9272edbfef3a66146b1e6c66507))
* **store:** normalize local paths before hashing in cache key ([8d2e849](https://github.com/PackWeave/weave/commit/8d2e849d30f758eb55b10606170679fff3159ab4)), closes [#133](https://github.com/PackWeave/weave/issues/133)
* **store:** stabilize list_cached sort order and improve test names ([1ea928c](https://github.com/PackWeave/weave/commit/1ea928c311ce97000e7ed0c31c7fb4ddd1c9d28c))
* **tests:** address review findings for CompositeRegistry tests ([d1e2efa](https://github.com/PackWeave/weave/commit/d1e2efab72e1760da9905d98c70b330c6900c375))
* **tests:** replace MockComposite with real CompositeRegistry in tests ([12b85bf](https://github.com/PackWeave/weave/commit/12b85bf2e54846793040065082abf21bc0f49c41))
* **tests:** replace MockComposite with real CompositeRegistry in tests ([24b3096](https://github.com/PackWeave/weave/commit/24b3096f7f5e8267c14a5734baba161adfb42f97)), closes [#142](https://github.com/PackWeave/weave/issues/142)
* **tests:** update eviction assertion to match WeaveError message format ([9738264](https://github.com/PackWeave/weave/commit/973826495825d7bfdddbda643e65b4529247796a))
* **tests:** use variant matching and clearer names per review ([04162fe](https://github.com/PackWeave/weave/commit/04162fe0e32b3aec2280cb1e2a1475ffbc189906))
