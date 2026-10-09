# Decisões técnicas

- **Três crates Rust.** `luvyn-core` contém linguagem, semântica, grafo, consultas, formato binário, edição e export. `luvyn` contém CLI e hosts HTTP/desktop; `luvyn-mobile` adapta o mesmo Core para o cliente Android. Filesystem e sessão semântica vivem no Core. Não há parser ou resolver alternativo em TypeScript.
- **IDE local com Monaco.** A mesma aplicação abre no navegador ou na janela nativa Wry/Tao e funciona offline depois de instalada. Isso entrega editor real, undo/redo, busca, completion, rename, diff e acessibilidade, mantendo execução semântica em Rust. O binário distribui frontend e workers, sem CDN.
- **Gramática orientada a linhas.** Declarações/seções na coluna 1, conteúdo indentado, assinaturas simples e texto livre preservado. Sem expressões, macros, avaliação ou significado implícito em prosa. Referências explícitas geram grafo determinístico.
- **IDs de conteúdo semântico.** Hash de kind + nome qualificado, independente da localização e redação do documento. A mudança do contrato não muda a identidade do símbolo, mas rename e namespace mudam.
- **Imports mínimos.** Mesmo módulo e nomes únicos resolvem sem boilerplate. Imports, aliases e nomes qualificados desambiguam; autocomplete usa nomes qualificados quando há colisão e Quick Fix apresenta alternativas sem escolher silenciosamente.
- **Binário com diretório e registros separados.** Postcard oferece serialização compacta; diretório persistido permite lookup e busca textual sem carregar cada corpo. CRC32, magic, versão, offsets e limites tornam corrupção um erro previsível. Sem memory mapping inseguro ou desserialização de ponteiros.
- **Cache de documentos.** Hash BLAKE3 evita reparsing de fontes idênticas, inclusive entre builds. Alterações de declarações exigem resolução/reindexação global linear para refletir dependentes corretamente. A sessão atualiza o modelo após debounce e mantém build persistente separado dos buffers.
- **Contexto sem adivinhar regras.** Estrutura e assinaturas são compactadas; duplicação é removida. Texto livre permanece intacto. Traduzir automaticamente uma regra jurídica/técnica para palavras menores poderia alterar o contrato.
- **Queries locais.** Nomes, fuzzy search, postings, padrões naturais limitados, BFS e travessia reversa. O orçamento textual é uma estimativa de caracteres; não depende do tokenizer ou API de um fornecedor.
- **Conflitos conservadores.** Buffers locais não são sobrescritos por watcher. Saves exigem hash de disco; rename é simulado e rejeitado quando introduz errors. Diretórios só podem ser removidos quando vazios.
- **Evolução.** Kinds/relações em string permitem extensão do vocabulário; alterações no schema estrutural exigem novo major de `.lu`. `source`/metadata preservam ligações futuras com código; editor services já usam UTF-16.
- **Fields e self.** Campos são símbolos filhos (`kind=field`, relação `contains`) no schema binário v2. Referências em prosa estrutural recebem localização exata; métodos compartilham o proprietário. Não há avaliação de atribuições. Rename usa as mesmas transações validadas do editor.
- **Catálogo único.** Vocabulário e documentação estruturada residem em `luvyn-core::language`; CLI, IDE, snippets e highlighting recebem essas entradas. Adicionar uma entrada ao catálogo não exige duplicar descrição no frontend.
- **Hosts separados.** Sessão compartilhada em `luvyn-core::ide`; HTTP/watcher em `server.rs`; lifecycle/targets em `ide/host.rs`; Wry/Tao em `ide/desktop.rs`. Desktop mantém o backend na vida da janela. Server escuta em loopback sem token de sessão ou validação de `Origin`; Server pertence ao terminal, sem daemon ou registro de PID.
- **Layout ortogonal.** ELK em worker organiza camadas e rotas Manhattan com portas fixas e lanes. Rendering fica limitado a 120 nodes/600 relações, com aviso e filtros; seleção reduz contraste do restante.
- **Build coordenado.** Progresso vem do pipeline do core. Guard da IDE evita requisições duplicadas; lock no workspace coordena escrita entre CLI e hosts. `.lu` continua saída oficial, com API pública `LuReader` independente do Compiler.



- Módulos derivam de paths relativos às source roots; símbolos não substituem módulos. `namespace` é override explícito legado; `module <name> override` é override avançado.
- Workspace separa documentation/target/artifact; cache é local à documentação e lock coordena escrita no target. Target deve existir. Sources continuam protegidas por safe_path.
- Tipos documentais adotam nomes Rust e AST de tipos recursivos com limites. `true`/`false` são literals. `Option<T>` substitui `T?`; `->` é retorno oficial.
- Android usa WebViewAssetLoader + JNI, mantendo o host existente Wry/HTTP. UI de toque usa CodeMirror; serviços, lexer e dictionary são Rust compartilhado. Tauri não foi introduzido para evitar trocar hosts funcionando.
- SAF pertence ao host Android; provider documents são staged em armazenamento privado, com target privado fornecido ao Core. O host publica somente `.lu` no target externo.

- **DSL genérica.** Cinco declarações, annotations livres, generics locais, export para APIs e main único. Papéis arquiteturais não aumentam o vocabulário. Nomes únicos dispensam imports; ambiguidades exigem decisão explícita.
- **Artifact v2.** Annotations e generics são metadata tipada no node; mudança estrutural exige major novo e rebuild, sem fingir compatibilidade com v1.
- **Criação e atualização.** IDE persiste create_new/sync antes de abrir o buffer; saves conferem hash real. Watcher ignora leituras; diagnostics idênticos não alteram markers do editor.
