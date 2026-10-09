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
cargo install --path crates/luvyn --features desktop --target-dir target --locked
```

O executável instalado em `~/.cargo/bin` inclui todos os assets da IDE. Node/npm são necessários somente para construir o frontend; o uso normal exige apenas `luvyn`. Se preferir um executável sem instalação, use `cargo build --release` e execute `target/release/luvyn` (`luvyn.exe` no Windows).

## Primeiro projeto

```sh
mkdir my-project-docs
cd my-project-docs
luvyn init --target ../my-project
luvyn .
```

Em distribuições com Desktop, `luvyn ide .` e `luvyn ide <path>` abrem a janela nativa diretamente no workspace; o backend usa uma porta dinâmica e encerra junto com a janela. `luvyn ide` abre Projetos. Use `luvyn ide --server` somente para abrir a IDE no navegador: o servidor permanece no terminal e encerra com Ctrl+C ou ao fechar o terminal, sem daemon ou registro de PID.

Desktop usa Wry/Tao e o WebView do sistema: WebView2 no Windows, WebKit no macOS, GTK3/WebKitGTK 4.1 no Linux. No Linux, instale as bibliotecas de desenvolvimento (`libgtk-3-dev libwebkit2gtk-4.1-dev libxdo-dev libdbus-1-dev` em Debian/Ubuntu) antes de compilar com `--features desktop`. Para somente server/CLI, omita essa feature; não são necessárias dependências gráficas. [Hosts e distribuição](docs/HOSTS.md).

Experimente o projeto incluído:

```sh
luvyn examples/users
luvyn check examples/users
luvyn build examples/users
luvyn get UserService --workspace examples/users
luvyn get "quem depende de UserRepository" --workspace examples/users
luvyn export examples/users
```

## Workspace e módulos

O projeto de documentação pode ter um repositório separado do código original:

```toml
sources = ["docs"]
output = ".luvyn/project.lu"

[project]
target = "../MyGame"
artifacts = "ignore"
```

`luvyn init` cria `luvyn.toml` sem sobrescrever configuração existente. `luvyn init --target ../MyGame` conecta um diretório existente. Paths relativos de target partem de `documentation_root`; paths absolutos também funcionam. Sem target, o próprio diretório da documentação recebe o artefato, preservando o fluxo integrado anterior.

`documentation_root` contém configuração e `.lyn`. `target_project_root` recebe o `.lu`; `artifact_root` é `<target>/.luvyn`. Caches da análise ficam na documentação; lock de build fica no target para coordenar documentação compartilhada. Output relativo é resolvido no target; exports opcionais continuam na documentação. Build nunca copia fontes `.lyn` ao código original.

```sh
cd MyGameDocs
luvyn check
luvyn build
# MyGame/.luvyn/project.lu
cd ../MyGame
luvyn get Player
```

Regra do módulo: path relativo à raiz de fontes mais específica, sem `.lyn`, separadores convertidos para `.`; segmentos consecutivos iguais colapsam. `docs/core.lyn` com `sources=["docs"]` gera `core`; `docs/user/service.lyn` gera `user.service`; `docs/auth/auth.lyn` gera `auth`. Case é preservado; segmentos devem ser identificadores válidos. Símbolos são `player.Player`; membros, `player.Player::health`. Nodes `module` e relações `contains` são gerados automaticamente.

O módulo é inferido automaticamente. Use `namespace users` para compartilhar um namespace entre arquivos ou `module game.players override` para substituir deliberadamente o módulo inferido. `module` não declara um símbolo.

## Linguagem

A DSL possui cinco declarações: `class`, `func`, `type`, `enum` e `interface`. Annotations livres descrevem papéis arquiteturais sem novas palavras-chave. Este documento pode ser copiado e compilado:

```lyn
@entity
class User
purpose: usuário da aplicação
fields:
    id: i64
    email: String
rules:
    - self.email deve ser único e válido

class CreateUserError
purpose: falha na criação de usuário

interface UserRepository<T>
purpose: persistência do tipo T
export:
    func save(value: T) -> T
    func find(id: i64) -> Option<T>

@service("users")
class UserService
purpose: gerenciar criação e consulta de usuários
depends:
    UserRepository
export:
    func create(name: String, email: String) -> Result<User, CreateUserError>
        validar email antes de persistir e registrar auditoria
    func find(id: i64) -> Option<User>
source:
    src/services/UserService.rs

main:
    purpose:
        backend responsável por usuários
    depends:
        UserRepository
    export:
        UserService
```

Declarações começam na coluna 1; conteúdo de blocos usa quatro espaços. Descrições de funções em `export:` usam oito espaços. `main:` contém blocos indentados com quatro espaços e seu conteúdo com oito. Pode haver **um único main em todo o projeto**; ele é a raiz inicial do grafo. Comentários usam `#` ou `//` fora de strings; URLs permanecem texto.

Blocos: `purpose`, `fields`, `rules`, `behavior`, `depends`, `export`, `notes`, `source`, `contracts` e `values` (enum). `purpose` reúne intenção e responsabilidade; `notes` guarda detalhes adicionais. `depends` registra consumo arquitetural, `export` identifica a API pública/documentada, `implements` identifica cumprimento de uma interface e `extends` registra especialização. Tipos de parâmetros e retorno geram automaticamente `uses`/`returns`; não escreva relações deriváveis de assinaturas.

Um arquivo `.resource.lyn` instancia uma classe existente com valores. Por exemplo, `Weapon.lyn` pode declarar `class Weapon` com fields `name: String`, `damage: i32` e `weight: f32`; `iron_sword.resource.lyn` fornece os valores:

```lyn
class Weapon "iron_sword"
purpose: arma inicial do jogador
fields:
    name: "Iron Sword"
    damage: 12
    weight: 2.5
```

O identificador entre aspas diferencia instâncias. O Core verifica os nomes e tipos dos fields, completa os que ainda faltam e grava a relação `instance_of` no grafo e no `.lu`. Resource aceita `purpose:`, `fields:` e `notes:`; regras, dependências, APIs e declarações continuam em arquivos `.lyn` normais.

Annotations aceitam `@name`, `@name()`, `@name(value)` e `@name(key: value)`, inclusive múltiplos argumentos e strings. Os nomes são livres e armazenados no grafo e no `.lu`. Não executam código. Exemplos: `@entity`, `@component`, `@service("users")`, `@system(order: 10)`, `@event` antes de `class`.

### Generics e tipos

```lyn
class Entity
purpose: identidade persistível

interface Repository<T: Entity>
purpose: abstrair persistência de um tipo
export:
    func find(id: i64) -> Option<T>
    func save(value: T) -> Result<T, RepositoryError>

class RepositoryError
purpose: falha de persistência

class ECS<T>
purpose: agrupar componentes do tipo T

class World<T>
purpose: estado do mundo
fields:
    components: ECS<T>
    values: Vec<Vec<Option<T>>>

func convert<T, R>(value: T) -> R
purpose: documentar conversão entre representações

type Collection<T> = Vec<T>
purpose: coleção ordenada de valores
```

Generics são locais à declaração; funções exportadas também enxergam os parâmetros do proprietário. Bounds simples (`T: Entity`) geram relação `bound`. Tipos aninhados e `>>` são aceitos; não há traits, lifetimes, execução ou borrow checker. `type` nomeia uma forma documental, não declara uma classe executável.

Tipos embutidos: `bool`, inteiros Rust (`i64`, `u32` etc.), `f32`, `f64`, `char`, `str`, `String`, `()`, `Option<T>`, `Result<T, E>`, `Vec<T>` e `HashMap<K, V>`. Outros nomes exigem declaração ou parâmetro genérico. `true`/`false` são literals, não tipos. Prefira `Option<User>` a `User?`; o formato legado opcional e retornos `: Type` ainda recebem warnings de migração.

`self.email` referencia somente um field do proprietário atual, inclusive em descrições de métodos. O autocomplete de `self.` lista apenas esses fields. Campo inexistente ou `self` em função standalone gera diagnostic; strings e comentários não geram referências. Rename de field atualiza declaração e referências sem modificar prosa livre.

### Referências e imports

Símbolos do mesmo módulo resolvem automaticamente. Um nome **único no projeto** também resolve sem import. Dois símbolos com o mesmo nome exigem nome qualificado ou import explícito; nenhuma escolha arbitrária é feita.

```lyn
# Pressupõe accounts.User e billing.User em outros documentos.
import accounts.User as Account
import billing.User as Customer

class Invoice
purpose: cobrar cliente e identificar operador
fields:
    operator: Account
    customer: Customer
```

Imports aceitam símbolos, `import users.*` e arquivos relativos (`import "users.lyn"`). Use-os para desambiguar ou explicitar fronteiras; evite boilerplate para símbolos únicos. Membros têm nomes como `users.UserService::create`. `source:` preserva mapeamentos para código real sem executar ou validar o caminho.

O catálogo do Core contém sintaxe, contextos, notas e exemplos copiáveis: `luvyn --lang`, `luvyn --lang generics`, `luvyn --lang @`, `luvyn --lang main` e `luvyn --lang --format json`. A IDE **Language**, autocomplete e highlighting usam a mesma fonte.

### Ignore e migração

`.ignore.luvyn` usa padrões de `.gitignore`, relativos ao workspace de documentação:

```gitignore
target/
generated/
legacy/**
*.generated.lyn
```

Arquivos excluídos não entram no Explorer, parser, index, autocomplete, grafo ou `.lu`; buffers ignorados também não são indexados. A configuração pode ser editada em **Settings → Editar .ignore.luvyn**. Source roots continuam configuráveis em `luvyn.toml`.

Declarações especializadas antigas (`entity`, `service`, `component`, `system`, `event`, `concept`, `struct`) agora geram erro de migração: use annotation + `class`. Troque `exposes`/`functions` por `export`, `responsibilities` por `purpose`, `flow` por `behavior` e `metadata` por annotations/`notes`. `emits`, `listens`, `link`, `related` e relações manuais `uses`/`returns`/`references` foram removidas. Artifacts antigos devem ser reconstruídos com `luvyn build`.

## CLI

| Comando | Uso |
| --- | --- |
| `luvyn [workspace]` | Abrir IDE; default `.` |
| `luvyn ide [workspace] --server` | Browser + backend no terminal; Ctrl+C encerra |
| `luvyn ide [workspace] --desktop` | Janela nativa; fecha seu backend junto com a janela |
| `luvyn ide [workspace] --foreground --no-open --port 7878` | Servidor em foreground para desenvolvimento |
| `luvyn ide build --server` / `--desktop` | Build da distribuição da IDE a partir deste repositório |
| `luvyn ide build --android` | Gera APK debug com Rust Core compartilhado |
| `luvyn --lang [keyword] [--format json]` | Dicionário oficial; alias `luvyn lang` |
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

O build descobre fontes respeitando `.gitignore`, `.ignore.luvyn`, `.luvynignore` e configuração própria. Não segue symlinks nem percorre `.git`, `.luvyn`, `target`, `node_modules`, `dist` e `graphify-out`. Cada fonte tem hash BLAKE3; documentos idênticos reutilizam sua representação parseada, inclusive entre execuções. Resolução global reconstrói relações ao mudar declarações; um arquivo inválido não substitui o último `.lu` válido. Escritas usam arquivo temporário e rename.

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
luvyn get "out:export UserService"
luvyn get "neighbors UserService"
luvyn get "path UserService -> User"
echo UserService | luvyn get --stdin
```

Busca por nome exato, nome qualificado, ID, substring, distância de edição e índice textual. Consultas naturais simples são interpretadas localmente em português/inglês. Não são compreensão geral de linguagem natural: use nomes, termos presentes nas fontes ou a DSL para resultados precisos. `in:RELATION X` recupera relações reversas; `out:RELATION X` diretas; `neighbors X` ambas; `path A -> B` encontra o menor caminho dirigido. Caminhos exigem endpoints únicos.

Default: profundidade 1, até 12 símbolos, formato `compact`. Também há `text`, `markdown`, `json`. A profundidade máxima é 8; o limite máximo é 500. `--budget` usa um orçamento conservador de caracteres (quatro por token estimado), não um tokenizer de um modelo específico. Texto truncado é explicitamente marcado; JSON preserva estrutura e é limitado por `--limit`/`--depth`, não pelo orçamento textual.

`get` consulta o último build. Se o artefato não existe, constrói automaticamente. Depois de editar fontes, execute `build` ou use `get QUERY --rebuild`. O diretório binário permite selecionar contexto pelo índice antes de deserializar somente os nodes relevantes.

### Export

`export` gera ZIP com `INDEX.md`, `modules/<namespace>.md` e `GLOBAL.md` para símbolos sem namespace. APIs são agrupadas sob seu dono; namespace aparece uma vez por documento. Assinaturas são compactadas (`func createUser(name: String, email: String) -> Option<User>`); listas/relacionamentos são deduplicados e estrutura repetitiva é removida. Texto livre, regras e contratos não são reinterpretados: uma substituição heurística de prosa pode alterar significado, então a compactação preserva a redação. Documentos e entradas são ordenados; timestamps e permissões são fixos. Fontes semanticamente idênticas geram o mesmo ZIP.

### Git e skill

`git` adiciona `*.lu`, `.luvyn/` e outputs personalizados quando necessário, sem duplicar entradas. Nunca ignora `.lyn`.

`skill` instala `skills/luvyn/SKILL.md` no diretório de skills de `CODEX_HOME`, se definido; caso contrário usa a convenção `~/.agents/skills`. Falha de instalação global usa automaticamente `.agents/skills` no workspace. `--local` força essa opção; `--directory PATH` permite escolher outro diretório de skills. Não sobrescreve uma skill diferente já existente. O preset ensina retrieval direcionado, relações, edição de `.lyn` e validação/build após mudanças. Reinicie/atualize a descoberta de skills do agente após instalar.

## IDE

Interface densa com temas escuro/claro, explorer, múltiplas abas, editor Monaco, outline, Problems, Output, busca textual no workspace, busca semântica, Command Palette e grafo interativo.

- Highlighting, auto-indent, pares de brackets/aspas, snippets, autocomplete do projeto e imports automáticos.
- Diagnostics inline, hover, F12, referências, rename e quick fixes para imports, nomes parecidos e declaração de conceito.
- Formatação, undo/redo, busca/substituição, marcador de alteração, save all e autosave configurável.
- Explorer real: criar documentos/pastas, renomear/mover, excluir arquivos/pastas vazias, filtrar e atualizar. Mover não reescreve imports relativos; diagnostics mostram referências que precisam ser atualizadas.
- Grafo ortogonal/Manhattan com ELK em Web Worker, camadas, portas fixas, lanes e labels. Seleção de node/edge destaca vizinhança e reduz contraste do restante. Preserva zoom/pan, minimap, filtros, profundidade, inspector e abertura da definição. Limite visual: 120 nodes/600 relações; foque contexto em projetos maiores.
- Build salva buffers, chama `Project::build_with_progress` fora da thread principal, mostra progresso/resultado no Output, atualiza Problems, `.lu` e grafo sem reiniciar. Mutex da sessão, guard da operação e lock do core impedem escrita concorrente do mesmo workspace entre CLI/server/desktop. Check reutiliza parsing, resolução e validação.
- Sidebar Language com busca, categorias, sintaxe, exemplos, contextos, aliases e entradas relacionadas, fornecidos pelo Core.
- Parsing após debounce de 450 ms reutiliza documentos inalterados. Trabalho Rust roda em `spawn_blocking`. O grafo e os índices são atualizados após edição, separados do build persistente.
- Watcher detecta mudanças externas. Arquivos limpos são recarregados; buffers modificados mostram conflito. O diff permite revisar/editar a versão local antes de salvar. O hash em disco é conferido novamente no save.

Atalhos: Ctrl+S salvar, Ctrl+Shift+B build, Ctrl+P símbolos, Ctrl+Shift+P comandos, Ctrl+J painel, Ctrl+F busca, Ctrl+H substituir, F12 definição, Shift+F12 referências, F2 rename, Shift+Alt+F formatar. As divisórias são redimensionáveis.

O servidor aceita somente conexões locais e não exige token de sessão nem valida `Origin`; abra diretamente o endereço `127.0.0.1` mostrado pelo host. Caminhos continuam limitados ao workspace; operações em internals e edição de symlinks são bloqueadas. Server pertence ao terminal e encerra com Ctrl+C ou ao fechar o terminal. Desktop encerra o backend ao fechar a janela. Não exponha essa porta através de proxy público.

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

## Formato `.lu` v2

Não é JSON renomeado. Header de 32 bytes contém magic `LUVYN\0\r\n`, versão major, flags, tamanho do diretório, tamanho dos dados, CRC32 do diretório e campo reservado. O diretório postcard contém descritores de nodes, offsets/tamanhos/checksums, arestas, hashes de fontes, diagnostics e postings do índice textual. Cada node tem payload postcard individual e CRC32. O loader valida versão, limites, IDs, relações e checksum antes de consumir dados. A API `Artifact::node` acessa um único registro; `Artifact::query` carrega os payloads selecionados pelo índice.

Kinds e relações são strings extensíveis. Campos estruturais novos exigem uma versão de formato futura; versões desconhecidas retornam erro com orientação para rebuild. CRC32 detecta corrupção acidental, não autentica arquivos recebidos de terceiros. Artefatos gerados devem permanecer fora do Git.

Saída oficial padrão: `.luvyn/project.lu` (configurável por `output`). Integrações usam `luvyn_core::LuReader`: `open`, `nodes`, `edges`, `source_hashes`, `node`, `query`, `load`. Essa API não depende do Compiler nem das fontes `.lyn`. `GraphNode`/`GraphEdge` são reexports dos tipos semânticos. [Contrato binário e exemplo de leitura](docs/LU_FORMAT.md). O schema v2 inclui annotations e parâmetros genéricos; leitores rejeitam v1 com erro estruturado. Recompile fontes antigas após migrar a linguagem.

## Organização e validação

```text
crates/luvyn-core/src/
    parser.rs       linguagem e diagnostics
    language.rs     catálogo oficial usado por todos os consumidores
    model.rs        símbolos, relações e índices
    resolver.rs     namespaces e imports
    workspace.rs    descoberta, configuração e cache
    binary.rs       formato .lu e acesso parcial
    query.rs        busca, traversal e contexto compacto
    editor.rs       serviços semânticos para edição
    ide.rs          sessão/ações compartilhadas por todos os hosts
    lexer.rs        tokens usados pelo editor mobile
    formatter.rs    formatação que preserva comentários
    export.rs       Markdown/ZIP determinístico
crates/luvyn/src/
    main.rs         CLI
    server.rs       adapter HTTP local e watcher
    ide/session.rs  reexport da sessão do Core
    ide/android.rs  build Android com Rust/NDK e Gradle
    ide/host.rs     targets, lifecycle e distribuição
    ide/desktop.rs  wrapper Wry/Tao
    ide/process.rs  detach e isolamento de handles da plataforma
    integration.rs git e preset Codex
ui/src/             Monaco, shell React e graph view
crates/luvyn-mobile/ bridge JNI para o mesmo Core Rust
apps/ide-mobile/    host Android, WebView local e workspace SAF
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

A IDE usa a mesma UI/backend nos hosts server e desktop Wry/Tao. Android possui host JNI/WebView e UI mobile própria. Distribuições são diretórios portáteis para a plataforma de build. Não há LSP externo ainda; serviços de edição já retornam ranges UTF-16 e edits reutilizáveis. A resolução global é linear e o cache incremental evita reparsing; alterações de estrutura ainda reindexam o grafo. Busca textual tem limite de 32 MiB por operação/200 resultados. Explorer prioriza `.lyn`, mas resultados da busca podem abrir outros arquivos de texto. Relação com código real permanece metadata. Rename é conservador: recusa projetos com errors, colisões e mudanças que quebrariam resolução; não reescreve prosa.

Licença MIT.


## Android / Mobile

Cliente real em `apps/ide-mobile`, com bridge JNI em `crates/luvyn-mobile`. O Rust Core fornece parser, tipos, lexer, diagnostics, editor, dictionary, graph, query e build. Desktop/Server usam a mesma sessão `luvyn-core::ide`. Mobile usa CodeMirror para toque e os mesmos serviços Rust; não há segunda implementação da linguagem.

```sh
rustup target add aarch64-linux-android
luvyn ide build --android
# dist/android/luvyn-debug.apk
```

Requisitos: JDK 17+, SDK platform 36/build-tools 36.0.0, NDK 27+ e Rust Android target. Configure `ANDROID_HOME`; `ANDROID_NDK_HOME` pode selecionar NDK específico. No Windows, o SDK padrão de Android Studio também é detectado. Gradle Wrapper 9.6.1 está incluído. Primeiro build precisa baixar dependências. Para emulator x86_64, defina `LUVYN_ANDROID_ABI=x86_64` e instale `rustup target add x86_64-linux-android`. Erros indicam o requisito ausente; `--output` escolhe diretório do APK.

Navegação: Files, Editor, Graph, Language, More. Files permite criar, abrir, renomear e excluir `.lyn`. Editor tem highlight do lexer Rust, diagnostics, autocomplete/`self.`, save, undo/redo, busca, símbolos e ajuste ao teclado. Graph usa foco obrigatório, profundidade 1 e limite 32 nodes; oferece pan, pinch, seleção e abrir definição. Dictionary é o mesmo do CLI. Problems em More abre arquivo/linha.

Android usa SAF para escolher Documentation e Target. Fontes são importadas para staging privado e alterações são sincronizadas ao provider da documentação. Configuração original é preservada; paths desktop de target não são usados no Android. O Core recebe somente paths privados comuns. Build gera artefato pelo mesmo pipeline e publica apenas `.luvyn/project.lu` no Target escolhido via SAF. Sem Target selecionado, Build pede seleção. Reimportação exige salvar alterações. Save verifica conflito externo antes de sobrescrever. Providers podem falhar durante escrita; erros são expostos e reimportação permite reconciliar. SAF não garante rename/escrita atômicos entre providers.

Primeira versão: Android 8+; APK debug não é release assinado. Imports limitados a 10.000 documentos/64 MiB e profundidade 32. Rename de arquivo não reescreve imports; diagnostics orientam correção. Diretórios privados de staging antigos são preservados nesta etapa para recuperação.
# Tela de Projetos

`luvyn ide --desktop` ou `luvyn ide --server` abre Projetos, sem exigir `.`. `luvyn ide .` e `luvyn ide <path>` abrem diretamente e registram recentes. Mobile sempre começa por Projetos. Local usa filesystem/SAF; Cloud usa Google Drive com cache local e o mesmo editor/Core.

Recentes persistem fora do workspace. Remover da lista preserva arquivos. Cloud sincroniza fontes/configuração pelo botão Sync e ao abrir; `.lu` permanece local. [Configuração OAuth, plataformas e conflitos](docs/PROJECTS.md).

