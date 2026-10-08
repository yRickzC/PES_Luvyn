---
name: luvyn
description: Retrieve focused technical documentation and graph context from a Luvyn workspace, or edit its .lyn documentation. Use when .lyn sources or a compiled Luvyn graph exist, or when the user explicitly requests Luvyn context.
---

Use the `luvyn` CLI from the workspace root. Prefer focused queries before opening many documentation files:

```sh
luvyn get UserService
luvyn get "in:depends UserRepository"
luvyn get "out:exposes UserService"
luvyn get "path UserService -> User"
luvyn get UserService --depth 2 --limit 16 --budget 2000
```

The default output is compact context. Use `--format json` when structured data helps. Increase depth or retrieve another subgraph only when the current context is insufficient. Natural queries such as `quem depende de X`, `dependências de X`, `quem usa X`, and `o que X expõe` resolve locally. Queries are deterministic and do not call an LLM.

`.lyn` files are editable source; `.lu` files are generated binary artifacts. Never edit `.lu`. The graph reflects the last build. After changing documentation, run `luvyn check` then `luvyn build`; use `luvyn get X --rebuild` to explicitly refresh context. Build automatically runs when no compiled artifact exists. Syntax and diagnostics include file and location; read the indicated source when intent is unclear.

Follow dependencies, exposed APIs, return types and reverse references to inspect change impact. Do not replace targeted retrieval with reading the whole project when the query resolves the task. Preserve contracts, rules and behavior when editing. Imports are explicit across namespaces. Keep generated graphs and `.luvyn/` cache out of Git with `luvyn git`; commit `.lyn` sources.

