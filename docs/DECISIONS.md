# Decisões técnicas

- **Dois crates Rust.** `luvyn-core` contém linguagem, semântica, grafo, consultas, formato binário, edição e export. `luvyn` contém CLI, filesystem da sessão e adapter HTTP da IDE. Não há parser ou resolver alternativo em TypeScript.
- **IDE local com Monaco.** A aplicação abre no navegador e funciona offline depois de instalada. Isso entrega editor real, undo/redo, busca, completion, rename, diff e acessibilidade, mantendo execução semântica em Rust. O binário distribui frontend e worker, sem CDN. Uma shell nativa pode reutilizar a mesma interface no futuro.
- **Gramática orientada a linhas.** Declarações/seções na coluna 1, conteúdo indentado, assinaturas simples e texto livre preservado. Sem expressões, macros, avaliação ou significado implícito em prosa. Referências explícitas geram grafo determinístico.
- **IDs de conteúdo semântico.** Hash de kind + nome qualificado, independente da localização e redação do documento. A mudança do contrato não muda a identidade do símbolo, mas rename e namespace mudam.
- **Imports explícitos.** Mesmo namespace tem visibilidade entre arquivos. Outros namespaces exigem import, alias, wildcard ou nome qualificado. O autocomplete fornece um edit adicional de import; ambiguidades são errors.
- **Binário com diretório e registros separados.** Postcard oferece serialização compacta; diretório persistido permite lookup e busca textual sem carregar cada corpo. CRC32, magic, versão, offsets e limites tornam corrupção um erro previsível. Sem memory mapping inseguro ou desserialização de ponteiros.
- **Cache de documentos.** Hash BLAKE3 evita reparsing de fontes idênticas, inclusive entre builds. Alterações de declarações exigem resolução/reindexação global linear para refletir dependentes corretamente. A sessão atualiza o modelo após debounce e mantém build persistente separado dos buffers.
- **Contexto sem adivinhar regras.** Estrutura e assinaturas são compactadas; duplicação é removida. Texto livre permanece intacto. Traduzir automaticamente uma regra jurídica/técnica para palavras menores poderia alterar o contrato.
- **Queries locais.** Nomes, fuzzy search, postings, padrões naturais limitados, BFS e travessia reversa. O orçamento textual é uma estimativa de caracteres; não depende do tokenizer ou API de um fornecedor.
- **Conflitos conservadores.** Buffers locais não são sobrescritos por watcher. Saves exigem hash de disco; rename é simulado e rejeitado quando introduz errors. Diretórios só podem ser removidos quando vazios.
- **Evolução.** Kinds/relações em string permitem extensão do vocabulário; alterações no schema estrutural exigem novo major de `.lu`. `source`/metadata preservam ligações futuras com código; editor services já usam UTF-16.

