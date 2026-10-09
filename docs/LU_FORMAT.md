# Artefato oficial `.lu` v2

`luvyn build` escreve `.luvyn/project.lu` por padrão, ou o caminho `output` do `luvyn.toml`. Build inválido preserva o último artefato válido. Fontes `.lyn` são editáveis; `.lu` é o grafo compilado para ferramentas externas.

Header fixo de 32 bytes, números little-endian:

| Offset | Tamanho | Conteúdo |
|---|---|---|
| 0 | 8 | Magic `LUVYN\0\r\n` |
| 8 | 2 | Major = 2 |
| 10 | 2 | Flags = 0 |
| 12 | 4 | Tamanho do diretório postcard |
| 16 | 8 | Tamanho dos registros de nodes |
| 24 | 4 | CRC32 do diretório |
| 28 | 4 | Reservado = 0 |

Após o header vêm diretório e registros. `Directory` em `binary.rs` define ordem/schema postcard: `records`, `edges`, `diagnostics`, `sources`, `text_index`. Records incluem ID, nome, nome qualificado, kind, offset relativo à seção de dados, tamanho e CRC32. Cada payload usa `model::Symbol`: identidade, namespace, parent, assinatura, annotations (nome, argumentos e localização), parâmetros genéricos (nome, bound e localização), seções semânticas, localização e end_line. Edges usam `model::Edge`, incluindo localização da referência. Postings indexam palavras para leitura parcial. Strings de kind/relação são extensíveis; O schema estrutural v2 substitui v1; leitores rejeitam versões anteriores. Execute `luvyn build` para reconstruir o artifact com a linguagem atual.

Offsets contíguos, checksums, versão, flags, índices, tamanho máximo, IDs e endpoints são validados. Limite de arquivo: 256 MiB. CRC32 detecta corrupção acidental. Integrações devem usar a API Rust em vez de implementar desserialização sem validação.

```rust,no_run
use luvyn_core::{LuReader, Result};
use std::path::Path;

fn inspect(path: &Path) -> Result<()> {
    let mut artifact = LuReader::open(path)?;
    let ids: Vec<_> = artifact.nodes().iter().map(|n| n.id.clone()).collect();
    for id in ids {
        if let Some(node) = artifact.node(&id)? {
            println!("{} {}", node.kind, node.qualified);
        }
    }
    for edge in artifact.edges() {
        println!("{} --{}--> {}", edge.from, edge.relation, edge.to);
    }
    Ok(())
}
```

`nodes()` lê apenas metadata do diretório; `node(id)` acessa um payload individual. `query` carrega apenas contexto selecionado; `load` retorna o grafo completo indexado. `source_hashes` permite verificar procedência sem acesso ao Compiler. `GraphNode`/`GraphEdge` são tipos públicos. O schema binário é versionado; consumidores devem rejeitar majors desconhecidos e reconstruir/reexportar, sem assumir compatibilidade silenciosa. Nenhum adapter Graphify externo foi instalado nesta etapa.

## Módulos e source mappings

O schema binário atual é v2. Nodes `module` representam módulos de arquivos, com `qualified` igual ao módulo e relações `contains` para símbolos de primeiro nível. Os demais nodes pertencem ao módulo por `namespace`; campos/métodos continuam ligados ao proprietário por `parent`. IDs continuam derivados de kind/nome qualificado. Mudanças de path/módulo alteram identidade; overrides explícitos estabilizam nomes quando necessário.

`sections["source"]` preserva mappings como `src/services/user.rs` e `src/services/user.rs::UserService`. São relativos ao projeto alvo por convenção, não caminhos da documentação. O Core preserva metadata sem ler ou parsear código real. Consultas parciais de `.lu` retornam esses mappings sem depender de `.lyn` no target.

Com `[project] target`, o arquivo default é `<target>/.luvyn/project.lu`; fontes e cache ficam na documentação. Os hashes em `sources` usam paths relativos à documentação. Um consumidor no target pode abrir diretamente `LuReader`/`Artifact` ou usar `luvyn get`. Novas semânticas exigem recompilar artefatos antigos; o cache de parsing usa versão 4 e é invalidado automaticamente.
