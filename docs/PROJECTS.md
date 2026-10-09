# Projetos locais e Google Drive

`luvyn ide`, `luvyn ide --desktop` e `luvyn ide --server` abrem a tela de Projetos. `luvyn ide .` ou `luvyn ide <path>` abrem diretamente o workspace e registram o acesso. Desktop mantém o backend dentro do processo da janela e o encerra ao fechar; Server permanece no terminal e encerra com Ctrl+C ou ao fechar o terminal, sem daemon ou registro de PID. Use o botão Projetos para trocar de workspace; primeiro salve os buffers modificados.

No Server, o caminho informado em Abrir Local pertence ao computador que executa o servidor. Novo projeto cria uma pasta inexistente, `main.lyn` e `luvyn.toml`; não sobrescreve projetos existentes. Remover dos recentes nunca apaga arquivos.

## Persistência e plataformas

Recentes armazenam somente ID, nome, localização, tipo, último acesso e ID Cloud opcional. Local indisponível fica desabilitado e pode ser removido da lista.

- Windows: `%LOCALAPPDATA%/Luvyn/projects.json`.
- macOS: `~/Library/Application Support/Luvyn/projects.json`.
- Linux: `$XDG_DATA_HOME/Luvyn/projects.json` ou `~/.local/share/Luvyn/projects.json`.
- `LUVYN_DATA_DIR` permite um diretório alternativo, inclusive em testes isolados.
- Android: armazenamento privado da aplicação. Projetos Local usam URIs persistentes SAF, não caminhos desktop. Novo Local solicita a pasta de destino e cria um subdiretório. Acesso revogado ou pasta removida deixa o recente indisponível. Mobile sempre inicia em Projetos, mesmo com workspace anterior selecionado.

O launcher possui uma pasta interna vazia para manter o host existente; ela não aparece nos recentes e nenhuma operação de documentação é executada antes de selecionar um projeto. Depois da abertura, todos os hosts usam a mesma sessão do Core. O watcher é transferido para o workspace selecionado.

## Configurar Google Drive

A integração não existia no código desta revisão e foi adicionada como adapter `luvyn-drive`, separado do compilador. Requer um projeto Google Cloud com Drive API habilitada e OAuth configurado.

### Desktop / Server

Crie um OAuth Client do tipo **Desktop app** e baixe seu JSON. Configure `LUVYN_GOOGLE_OAUTH_FILE` com o caminho absoluto desse arquivo ou coloque-o no diretório da aplicação como `google-oauth.json`. O JSON deve conter `installed.client_id` e, quando fornecido pelo Google, `installed.client_secret`. Nunca adicione esse arquivo ao repositório.

No Server, Conectar Google Drive abre uma nova aba no mesmo navegador da IDE; permita popups para o endereço local. No Desktop, o host abre o navegador do sistema, pois o Google não permite login em WebViews embutidas. O fluxo usa authorization code, PKCE S256, state aleatório e callback loopback com porta dinâmica; expira em três minutos. O refresh token fica no cofre nativo: Windows Credential Manager, macOS Keychain ou Linux Secret Service. Nenhum token é salvo em JSON, `.lu`, workspace ou frontend. Em Linux é necessário um serviço de secrets disponível. Se o cofre falhar, a autorização não é persistida em plaintext.

Se o servidor for encerrado, a tela avisa que perdeu a conexão e tenta recuperar enquanto ele estiver disponível no mesmo endereço. Ao reiniciar com porta dinâmica, abra a nova URL mostrada no terminal; uma aba com a porta anterior não pode se conectar ao novo processo.

### Android

Cadastre um OAuth Client **Android** para o package `dev.luvyn.mobile` e o SHA-1 da chave usada para assinar o APK. Debug e release podem exigir clients diferentes. Use o mesmo projeto Google Cloud que os clients Desktop. Não copie o JSON Desktop nem client secret para o APK.

A autorização usa Google Play Services AuthorizationClient com `drive.file`. Grants e conta permanecem sob gerenciamento dos serviços Google. O access token tem vida curta, fica somente em memória nativa e é obtido novamente antes de operações remotas; nunca passa para JavaScript nem é persistido em preferences. Dispositivos precisam dos serviços Google compatíveis.

### Escopo e identificação

O único scope solicitado é `https://www.googleapis.com/auth/drive.file`. A lista contém projetos criados/autorizados para a aplicação; esse scope não permite varrer indiscriminadamente todo o Drive. Pastas arbitrárias de documentação criadas fora do aplicativo não são importadas automaticamente.

Novo Cloud cria `Luvyn/<nome>/`. Cada pasta de projeto recebe as propriedades públicas `luvyn=project` e `schema=1`; a identificação usa seu ID e metadata, não o nome. Renomear a pasta no Drive não muda sua identidade. A API lista resultados paginados e preserva os IDs de cada documento.

## Sincronização

Abrir Cloud sincroniza o cache privado da aplicação. O botão **Sync** sincroniza alterações locais/remotas enquanto o projeto está aberto. A operação exige buffers salvos e aguarda Build. Não existe publicação automática a cada caractere.

São sincronizados `.lyn`, `luvyn.toml`, `.ignore.luvyn` e arquivos de ignore relacionados. `.lu`, `.luvyn`, caches, Git, `target`, `dist` e `node_modules` nunca são enviados. Build Cloud escreve artifacts locais; caminhos Target específicos de outro computador não controlam esse build. No Android um Target SAF selecionado pode receber o artifact, sem enviá-lo ao Drive.

O Core define `SyncProvider`; o adapter Google implementa listagem, criação, download, upload, atualização e remoção. O workspace local existente é a cópia de trabalho de ambos os tipos de projeto. `.luvyn/sync.json` contém apenas IDs/versões remotos e hashes BLAKE3 do último estado sincronizado.

O sync compara base, disco e remoto. Conteúdo inalterado não gera upload. Mudanças simultâneas divergentes geram conflitos antes de gravar; exclusões no Drive usam trash. O manifest `.luvyn/sync.json` persiste `last_synced_version` por arquivo e continua lendo manifests antigos. O adapter usa `fileId` e `version`, com `modifiedTime`, `md5Checksum` e `headRevisionId` quando disponíveis, conforme a [metadata oficial do Drive v3](https://developers.google.com/workspace/drive/api/reference/rest/v3/files). Antes de upload ou trash, consulta metadata e rejeita versões divergentes, sem depender de ETag. Downloads verificam metadata antes e depois da transferência. A versão retornada pelo PATCH é persistida; mudanças apenas remotas são baixadas. A verificação é otimista: não constitui uma operação atômica entre consulta e PATCH; escritores externos ainda podem alterar o arquivo nesse intervalo.

Conflitos aparecem com os paths envolvidos. Cache e remoto são preservados. Revise ambos os conteúdos pelo cache indicado e Drive, faça-os convergir e execute Sync novamente. Não há resolução automática nem editor de merge nesta tela. Erros de rede podem deixar operações já confirmadas sincronizadas; o manifest é persistido após cada operação para permitir retomada.

## Validação desta mudança

Somente testes focados de projetos/sync, abertura CLI, sessão de troca de projeto, contrato HTTP Drive e registry Mobile foram executados. Não foram rodadas suítes globais. Login real e leitura/escrita na conta Google requerem clients e autorização do usuário; testes de contrato locais não substituem essa validação.

Referências: [OAuth para Desktop](https://developers.google.com/identity/protocols/oauth2/native-app), [Authorization Android](https://developer.android.com/identity/authorization), [propriedades Drive](https://developers.google.com/workspace/drive/api/guides/properties), [scope Drive](https://developers.google.com/workspace/drive/api/guides/api-specific-auth).
