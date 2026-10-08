# Luvyn

**Documentação editável em `.lyn`. Contexto consultável em `.lu`.**

Luvyn é uma IDE local e uma CLI para documentar intenção, contratos, regras, APIs e relações de um projeto. O core Rust interpreta a linguagem, resolve referências, mantém índices e compila um grafo binário. A IDE utiliza esse mesmo core; consultas básicas não precisam de LLM, conexão externa ou API key.

## Instalação

Requisitos de desenvolvimento: Rust estável com suporte à edição 2024, Node.js 22.12+ e npm. No Windows, instale as ferramentas C++ do Visual Studio Build Tools usadas pelo target MSVC.

```sh
cd ui
npm ci
npm run build
cd ..
cargo install --path crates/luvyn --target-dir target
```

O executável instalado em `~/.cargo/bin` inclui todos os assets da IDE. Node/npm são necessários somente para construir o frontend; o uso normal exige apenas `luvyn`. Se preferir um executável sem instalação, use `cargo build --release` e execute `target/release/luvyn` (`luvyn.exe` no Windows).

## Primeiro projeto

```sh
mkdir my-project
cd my-project
luvyn .
```

A IDE abre no navegador padrão, conectada exclusivamente ao servidor Rust em `127.0.0.1`, numa porta disponível. Crie um `.lyn` pelo explorer; não há wizard obrigatório. Outra opção: escreva fontes em qualquer editor e use somente a CLI. Fechar a aba não encerra o processo: use Ctrl+C no terminal.

Experimente o projeto incluído:

```sh
luvyn examples/users
luvyn check examples/users
luvyn build examples/users
luvyn get UserService --workspace examples/users
luvyn get "quem depende de UserRepository" --workspace examples/users
luvyn export examples/users
```

## Linguagem

```lyn
namespace app.users

entity User
purpose: usuário da aplicação
fields:
    id: ID
    email: String
rules:
    - email único e válido

interface UserRepository
purpose: contrato de persistência
exposes:
    persist(user: User) -> User
    findById(id: ID) -> User?

service UserService
implements UserRepository

purpose:
    gerenciar usuários
rules:
    - criação exige email válido
    - escrita registra auditoria
behavior:
    validar email antes de persistir
flow:
    createUser: validate_email -> repository.persist -> audit
depends:
    UserRepository
exposes:
    createUser(name: String, email: String) -> User?
    getUser(id: ID) -> User?
source:
    src/services/UserService.java
```

Declarações começam na coluna 1. Seções também; seu conteúdo recebe quatro espaços. Seções aceitam conteúdo inline (`purpose: intenção`) ou várias linhas indentadas. Listas podem usar `- `. Uma declaração termina na próxima declaração. Arquivos podem conter vários símbolos. Um namespace opcional aparece antes das declarações. Comentários usam `#` ou `//`; dentro de aspas e em URLs, esses caracteres permanecem texto.

Kinds: `class`, `service`, `interface`, `component`, `system`, `entity`, `module`, `func`/`function`, `event`, `struct`, `enum`, `concept`.

Texto estruturado: `purpose`, `rules`, `behavior`, `responsibilities`, `contracts`, `fields`, `values`, `source`, `metadata`, `notes`, `flow`.

Relações: `depends`, `implements`, `uses`, `returns`, `emits`, `listens`, `references`, `extends`, `related`. Use `depends Target`, `depends: Target` ou uma lista em `depends:`. Relações próprias usam `link relação Target`:

```lyn
service Checkout
purpose: finalizar compra
link owns Order
```

`Order` deve ser um símbolo declarado. `exposes:` cria nodes de função filhos com arestas `exposes`. Assinaturas são `name(parameter: Type, ...) -> Type`; `: Type` após `)` também funciona. `?`, `[]` e tipos genéricos simples, como `List<User>`, são aceitos. Parâmetros geram relações `uses`; tipos de retorno geram `returns`. Tipos básicos (`String`, `ID`, `Bool`, `Int`, `Float`, `Void`, `Date`, `Any`, `List`, `Map`, `Result`, etc.) não precisam de declaração. Não existem expressões executáveis ou avaliação de código.

### Referências e imports

Símbolos no mesmo namespace são visíveis entre arquivos. Outros namespaces exigem import ou nome qualificado:

```lyn
namespace app.auth
import app.users.UserService
import app.users.User as Account
# Alternativas: import app.users.* ou import "../users/User.lyn"

service AuthService
purpose: coordenar autenticação
depends UserService
exposes:
    authenticate(email: String) -> Account?
```

APIs expostas possuem nomes qualificados como `app.users.UserService::createUser`. Imports de arquivo são relativos ao documento. Referências duplicadas/ambíguas produzem diagnostics, nunca uma escolha arbitrária. O autocomplete pode inserir imports de símbolos de outros namespaces. Campos (`id: ID`) e valores de enum permanecem texto semântico. `source:` é metadata de mapeamento para código real; não valida nem executa o arquivo indicado.

## CLI

| Comando | Uso |
| --- | --- |
| `luvyn [workspace]` | Abrir IDE; default `.` |
| `luvyn ide [workspace] --no-open --port 7878` | Servidor local sem abrir navegador |
| `luvyn check [workspace] [--format json]` | Sintaxe, imports, nomes duplicados e referências |
| `luvyn build [workspace]` | Compilar para `.luvyn/project.lu` |
| `luvyn get QUERY [--workspace DIR]` | Consultar artefato compilado |
| `luvyn export [workspace] [-o context.zip]` | Build e ZIP Markdown |
| `luvyn fmt [workspace] [--check]` | Formatação estável, preservando comentários |
| `luvyn fmt --stdin` | Formatar stdin e emitir stdout |
| `luvyn git [workspace]` | Configurar `.gitignore` idempotente |
| `luvyn skill [workspace] [--local]` | Instalar preset Codex |

Cada comando possui `--help`. Sucesso retorna código 0; erro retorna 1; uso inválido da CLI retorna 2. Dados JSON vão para stdout; diagnostics/logs vão para stderr. `check` valida sem produzir um artefato final. O lint alerta quando um símbolo não tem `purpose`.

### Build e cache

O build descobre fontes respeitando `.gitignore`, `.luvynignore` e configuração própria. Não segue symlinks nem percorre `.git`, `.luvyn`, `target`, `node_modules`, `dist` e `graphify-out`. Cada fonte tem hash BLAKE3; documentos idênticos reutilizam sua representação parseada, inclusive entre execuções. Resolução global reconstrói relações ao mudar declarações; um arquivo inválido não substitui o último `.lu` válido. Escritas usam arquivo temporário e rename.

IDs derivam de kind e nome qualificado. Mover arquivos, alterar comentários ou reescrever propósito preserva IDs. Rename, mudança de namespace ou kind cria uma nova identidade. Ciclos são permitidos; consultas usam conjunto de visitados. Limites: 2 MiB por fonte, 100.000 documentos, 256 MiB por `.lu`. Erros de UTF-8, IO, versão e corrupção retornam erros estruturados.

### Get: contexto focado

```sh
luvyn get UserService
luvyn get createUser --format json
luvyn get UserService --depth 2 --limit 16 --budget 2000
luvyn get "quem depende de UserRepository"
luvyn get "dependências de UserService"
luvyn get "o que UserService expõe"
luvyn get "quem usa User"
luvyn get "onde User é usado"
luvyn get "fluxo de criação de usuário"
luvyn get "como UserService cria usuário"
luvyn get "in:depends UserRepository"
luvyn get "out:exposes UserService"
luvyn get "neighbors UserService"
luvyn get "path UserService -> User"
echo UserService | luvyn get --stdin
```

Busca por nome exato, nome qualificado, ID, substring, distância de edição e índice textual. Consultas naturais simples são interpretadas localmente em português/inglês. Não são compreensão geral de linguagem natural: use nomes, termos presentes nas fontes ou a DSL para resultados precisos. `in:RELATION X` recupera relações reversas; `out:RELATION X` diretas; `neighbors X` ambas; `path A -> B` encontra o menor caminho dirigido. Caminhos exigem endpoints únicos.

Default: profundidade 1, até 12 símbolos, formato `compact`. Também há `text`, `markdown`, `json`. A profundidade máxima é 8; o limite máximo é 500. `--budget` usa um orçamento conservador de caracteres (quatro por token estimado), não um tokenizer de um modelo específico. Texto truncado é explicitamente marcado; JSON preserva estrutura e é limitado por `--limit`/`--depth`, não pelo orçamento textual.

`get` consulta o último build. Se o artefato não existe, constrói automaticamente. Depois de editar fontes, execute `build` ou use `get QUERY --rebuild`. O diretório binário permite selecionar contexto pelo índice antes de deserializar somente os nodes relevantes.

### Export

`export` gera ZIP com `INDEX.md`, `modules/<namespace>.md` e `GLOBAL.md` para símbolos sem namespace. APIs são agrupadas sob seu dono; namespace aparece uma vez por documento. Assinaturas são compactadas (`User? createUser name:String email:String`); listas/relacionamentos são deduplicados e estrutura repetitiva é removida. Texto livre, regras e contratos não são reinterpretados: uma substituição heurística de prosa pode alterar significado, então a compactação preserva a redação. Documentos e entradas são ordenados; timestamps e permissões são fixos. Fontes semanticamente idênticas geram o mesmo ZIP.

### Git e skill

`git` adiciona `*.lu`, `.luvyn/` e outputs personalizados quando necessário, sem duplicar entradas. Nunca ignora `.lyn`.

`skill` instala `skills/luvyn/SKILL.md` no diretório de skills de `CODEX_HOME`, se definido; caso contrário usa a convenção `~/.agents/skills`. Falha de instalação global usa automaticamente `.agents/skills` no workspace. `--local` força essa opção; `--directory PATH` permite escolher outro diretório de skills. Não sobrescreve uma skill diferente já existente. O preset ensina retrieval direcionado, relações, edição de `.lyn` e validação/build após mudanças. Reinicie/atualize a descoberta de skills do agente após instalar.

## IDE

Interface densa com temas escuro/claro, explorer, múltiplas abas, editor Monaco, outline, Problems, Output, busca textual no workspace, busca semântica, Command Palette e grafo interativo.

- Highlighting, auto-indent, pares de brackets/aspas, snippets, autocomplete do projeto e imports automáticos.
- Diagnostics inline, hover, F12, referências, rename e quick fixes para imports, nomes parecidos e declaração de conceito.
- Formatação, undo/redo, busca/substituição, marcador de alteração, save all e autosave configurável.
- Explorer real: criar documentos/pastas, renomear/mover, excluir arquivos/pastas vazias, filtrar e atualizar. Mover não reescreve imports relativos; diagnostics mostram referências que precisam ser atualizadas.
- Grafo com zoom/pan, minimap, seleção, filtros de kind/relação, profundidade, vizinhança, inspector e abertura da definição por duplo clique. A visualização fica limitada a 120 nodes; pesquise um símbolo para navegar em projetos maiores.
- Parsing após debounce de 450 ms reutiliza documentos inalterados. Trabalho Rust roda em `spawn_blocking`. O grafo e os índices são atualizados após edição, separados do build persistente.
- Watcher detecta mudanças externas. Arquivos limpos são recarregados; buffers modificados mostram conflito. O diff permite revisar/editar a versão local antes de salvar. O hash em disco é conferido novamente no save.

Atalhos: Ctrl+S salvar, Ctrl+Shift+B build, Ctrl+P símbolos, Ctrl+Shift+P comandos, Ctrl+J painel, Ctrl+F busca, Ctrl+H substituir, F12 definição, Shift+F12 referências, F2 rename, Shift+Alt+F formatar. As divisórias são redimensionáveis.

O servidor aceita somente conexões locais. Cada sessão exige um token aleatório em header e valida Origin quando presente. O token é entregue no fragmento da URL, removido da barra após abrir. Caminhos são limitados ao workspace; operações em internals e edição de symlinks são bloqueadas. Preserve o processo Rust aberto durante a sessão. Não exponha essa porta através de proxy público.

## Configuração opcional

Tudo funciona sem configuração. `luvyn.toml` pode restringir raízes/outputs:

```toml
sources = ["docs", "architecture"]
ignore = ["drafts/**"]
output = ".luvyn/project.lu"
export = ".luvyn/context.zip"
autosave = false
```

Outputs precisam ficar dentro do workspace. `.luvynignore` usa padrões semelhantes a `.gitignore`. A configuração deste repositório inclui `docs` e `examples` para dogfooding.

## Formato `.lu` v1

Não é JSON renomeado. Header de 32 bytes contém magic `LUVYN\0\r\n`, versão major, flags, tamanho do diretório, tamanho dos dados, CRC32 do diretório e campo reservado. O diretório postcard contém descritores de nodes, offsets/tamanhos/checksums, arestas, hashes de fontes, diagnostics e postings do índice textual. Cada node tem payload postcard individual e CRC32. O loader valida versão, limites, IDs, relações e checksum antes de consumir dados. A API `Artifact::node` acessa um único registro; `Artifact::query` carrega os payloads selecionados pelo índice.

Kinds e relações são strings extensíveis. Campos estruturais novos exigem uma versão de formato futura; versões desconhecidas retornam erro com orientação para rebuild. CRC32 detecta corrupção acidental, não autentica arquivos recebidos de terceiros. Artefatos gerados devem permanecer fora do Git.

## Organização e validação

```text
crates/luvyn-core/src/
    parser.rs       linguagem e diagnostics
    model.rs        símbolos, relações e índices
    resolver.rs     namespaces e imports
    workspace.rs    descoberta, configuração e cache
    binary.rs       formato .lu e acesso parcial
    query.rs        busca, traversal e contexto compacto
    editor.rs       serviços semânticos para edição
    formatter.rs    formatação que preserva comentários
    export.rs       Markdown/ZIP determinístico
crates/luvyn/src/
    main.rs         CLI
    server.rs       adapter HTTP local e watcher
    integration.rs git e preset Codex
ui/src/             Monaco, shell React e graph view
docs/               documentação do próprio Luvyn em .lyn
examples/users/     projeto funcional independente
skills/luvyn/       preset Codex distribuído com CLI
```

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cd ui
npm run build
npm audit --omit=dev
cd ..
luvyn check
luvyn build
luvyn get Compiler
luvyn export
```

Testes verificam parser, imports/ambiguidade, IDs estáveis, queries/ciclos/caminhos, roundtrip e corrupção binária, acesso parcial, ZIP determinístico, cache incremental, rename e UTF-16, IO inválido e fluxos reais da CLI. `docs/*.lyn` documenta o compilador, grafo, IDE e exporter e participa das mesmas consultas que os exemplos.

## Limites desta versão

A IDE é uma aplicação local no navegador, sem janela nativa Tauri. Não há LSP externo ainda; serviços de edição já retornam ranges UTF-16 e edits reutilizáveis. A resolução global é linear e o cache incremental evita reparsing; alterações de estrutura ainda reindexam o grafo. Busca textual tem limite de 32 MiB por operação/200 resultados. Explorer prioriza `.lyn`, mas resultados da busca podem abrir outros arquivos de texto. Relação com código real permanece metadata. Rename é conservador: recusa projetos com errors, colisões e mudanças que quebrariam resolução; não reescreve prosa.

Licença MIT.

