//! Authoritative vocabulary and copyable examples consumed by parser, CLI and IDE.
use serde::Serialize;
use std::sync::LazyLock;
pub const KINDS: &[&str] = &["class", "func", "type", "enum", "interface"];
pub const TEXT_SECTIONS: &[&str] = &[
    "purpose",
    "fields",
    "rules",
    "behavior",
    "contracts",
    "values",
    "source",
    "notes",
];
pub const RELATIONS: &[&str] = &["depends", "implements", "extends"];
pub const STATE_KINDS: &[&str] = &["class", "interface", "type"];
pub const SELF_SECTIONS: &[&str] = &["purpose", "rules", "behavior", "contracts", "notes"];
pub const BUILTINS: &[&str] = &[
    "bool", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64", "char", "str", "String", "()", "Option", "Result", "Vec", "HashMap",
];
pub const LITERALS: &[&str] = &["true", "false"];
pub const GENERICS: &[(&str, usize)] = &[("Option", 1), ("Result", 2), ("Vec", 1), ("HashMap", 2)];
#[derive(Clone, Debug, Serialize)]
pub struct LanguageEntry {
    pub keyword: String,
    pub category: String,
    pub syntax: String,
    pub description: String,
    pub contexts: Vec<String>,
    pub examples: Vec<String>,
    pub notes: String,
    pub aliases: Vec<String>,
    pub related: Vec<String>,
    pub completion: String,
}
const USERS: &str = "class User\npurpose: identificar usuário\nfields:\n    id: i64\n    email: String\n\nclass CreateUserError\npurpose: descrever falha na criação\n\ninterface UserRepository\npurpose: contrato de persistência\nexport:\n    func find(id: i64) -> Option<User>\n\n@service(\"users\")\nclass UserService\npurpose:\n    gerenciar criação e consulta de usuários\ndepends:\n    UserRepository\nexport:\n    func create(name: String, email: String) -> Result<User, CreateUserError>\n    func find(id: i64) -> Option<User>\n";
const REPOSITORY: &str = "class RepositoryError\npurpose: falha de persistência\n\ninterface Repository<T>\npurpose:\n    abstrair persistência de um tipo\nexport:\n    func find(id: i64) -> Option<T>\n    func save(value: T) -> Result<T, RepositoryError>\n";
static DICTIONARY: LazyLock<Vec<LanguageEntry>> = LazyLock::new(|| {
    let mut entries = Vec::new();
    macro_rules! row {
        ($key:expr, $category:expr, $syntax:expr, $description:expr, $context:expr, $example:expr, $notes:expr, $related:expr, $completion:expr) => {
            entries.push(LanguageEntry {
                keyword: $key.into(),
                category: $category.into(),
                syntax: $syntax.into(),
                description: $description.into(),
                contexts: vec![$context.into()],
                examples: vec![$example.into()],
                notes: $notes.into(),
                aliases: vec![],
                related: $related.iter().map(|s: &&str| (*s).into()).collect(),
                completion: $completion.into(),
            });
        };
    }
    row!(
        "class",
        "Declarations",
        "class Name<T: Bound>",
        "Documenta um objeto, serviço ou conceito com estado e API.",
        "arquivo",
        USERS,
        "Use annotations para caracterizar entidades, componentes e sistemas. Não implica execução ou uma linguagem específica.",
        &["@", "fields", "export"],
        "class ${1:Name}\npurpose:\n    ${2:intent}"
    );
    row!(
        ".resource.lyn",
        "Structure",
        "class Schema \"identity\"",
        "Instancia uma class existente com valores de dados validados pelo schema.",
        "arquivo *.resource.lyn",
        "class Weapon \"iron_sword\"\npurpose: arma inicial do jogador\nfields:\n    name: \"Iron Sword\"\n    damage: 12\n    weight: 2.5\n",
        "O arquivo precisa terminar em .resource.lyn. Use class Weapon \"iron_sword\": Weapon é uma class definida em um arquivo .lyn separado, com name: String, damage: i32 e weight: f32; iron_sword identifica a instância. Permitidos: purpose:, fields: com valores e notes:. Field inexistente ou valor incompatível gera diagnostic. Use vários arquivos resource para instâncias separadas; funções, relações e declarações de comportamento pertencem aos .lyn.",
        &["class", "fields", "purpose", "instance_of"],
        "class ${1:Schema} \"${2:resource_id}\"\npurpose:\n    ${3:why this data exists}\nfields:\n    ${4:field}: ${5:value}"
    );
    row!(
        "func",
        "Declarations",
        "func name<T>(arg: Type) -> ReturnType",
        "Documenta uma função; dentro de export documenta um método público.",
        "arquivo ou export",
        "func convert<T, R>(value: T) -> R\npurpose: converter um valor entre representações\n",
        "O corpo é descrição, não código executável. O retorno é opcional; use () quando quiser documentar ausência de valor.",
        &["export", "generics", "behavior"],
        "func ${1:name}(${2:arg}: ${3:String}) -> ${4:()}"
    );
    row!(
        "type",
        "Declarations",
        "type Name<T> = OtherType",
        "Nomeia um tipo ou alias de uma estrutura de dados.",
        "arquivo",
        "type Collection<T> = Vec<T>\npurpose: coleção ordenada de elementos\n",
        "Use class para objetos com responsabilidades; type para vocabulário de dados. Aliases preservam referências a seus tipos sem expandi-los recursivamente.",
        &["class", "generics", "fields"],
        "type ${1:Name} = ${2:i64}"
    );
    row!(
        "enum",
        "Declarations",
        "enum Name",
        "Documenta um conjunto de alternativas.",
        "arquivo",
        "enum UserStatus\npurpose: estado de acesso do usuário\nvalues:\n    Active\n    Suspended\n",
        "Use values para alternativas; não crie classes quando o conceito é um conjunto fechado de nomes.",
        &["values", "type"],
        "enum ${1:Name}\npurpose:\n    ${2:intent}\nvalues:\n    ${3:Value}"
    );
    row!(
        "interface",
        "Declarations",
        "interface Name<T>",
        "Documenta um contrato de API sem implementar comportamento.",
        "arquivo",
        REPOSITORY,
        "Use implements para ligar uma classe ao contrato. Use class quando o documento descreve estado e implementação.",
        &["implements", "export", "contracts"],
        "interface ${1:Name}\npurpose:\n    ${2:contract}"
    );
    row!(
        "@",
        "References",
        "@name | @name() | @name(value) | @name(key: value)",
        "Classifica o próximo símbolo com metadata extensível.",
        "antes de declaração ou método exportado",
        "@entity\nclass Player\npurpose: jogador da partida\n\n@component()\nclass Health\npurpose: estado de vida\nfields:\n    value: f32\n\n@service(\"users\")\n@system(order: 10)\nclass MovementSystem\npurpose: atualizar posições\n",
        "Qualquer nome válido é aceito. Argumentos são valores documentais, preservados sem avaliação; não geram dependências implícitas. Várias annotations podem preceder um símbolo.",
        &["class", "type", "generics"],
        "@${1:annotation}"
    );
    row!(
        "generics",
        "Types",
        "<T, R> | <T: SomeType>",
        "Declara parâmetros de tipo locais, com bounds simples opcionais.",
        "class, interface, func e type",
        REPOSITORY,
        "T resolve apenas no símbolo e seus fields/métodos. Bounds resolvem símbolos do projeto; nomes de parâmetros não exigem import. Sem traits, lifetimes ou avaliação de tipos.",
        &["type", "func", "interface"],
        "<${1:T}>"
    );
    row!(
        "main",
        "Structure",
        "main:\n    purpose:\n        ...\n    depends:\n        Symbol\n    export:\n        Symbol",
        "Define a documentação raiz do projeto, opcional e única.",
        "arquivo / coluna 1",
        "class Database\npurpose: persistir dados\n\nclass Cache\npurpose: acelerar consultas\n\nclass UserService\npurpose: gerenciar usuários\n\nmain:\n    purpose:\n        backend responsável por usuários\n    depends:\n        Database\n        Cache\n    export:\n        UserService\n",
        "Use para descrever o sistema como um todo; não substitui cada classe. Dois main, mesmo em arquivos diferentes, geram erro. O grafo abre pela raiz quando ela existe.",
        &["purpose", "depends", "export"],
        "main:\n    purpose:\n        ${1:project intent}"
    );
    for (key, description, example, notes) in [
        (
            "purpose",
            "Concentra intenção e responsabilidades do símbolo.",
            USERS,
            "Use para o que o símbolo faz; detalhes auxiliares ficam em notes. Substitui responsibilities.",
        ),
        (
            "fields",
            "Declara estado tipado e cria nodes field pertencentes ao símbolo.",
            "class World<T>\npurpose: guardar elementos da simulação\nfields:\n    values: Vec<T>\n",
            "Use para estado, não para parâmetros de funções. Campos duplicados são erros.",
        ),
        (
            "rules",
            "Documenta invariantes e restrições.",
            "class User\npurpose: identidade do usuário\nfields:\n    email: String\nrules:\n    - self.email deve ser único\n",
            "Use para condições que sempre devem valer. Sequência operacional pertence a behavior.",
        ),
        (
            "behavior",
            "Descreve fluxo e comportamento sem executar código.",
            "class User\npurpose: identidade do usuário\nfields:\n    name: String\nexport:\n    func setName(name: String) -> User\n        self.name = name\nbehavior:\n    validar nome antes de persistir\n",
            "Use para sequência e efeitos; regras permanentes ficam em rules. Descrições de métodos usam oito espaços.",
        ),
        (
            "contracts",
            "Documenta pré-condições, pós-condições e garantias de API.",
            "interface Validator\npurpose: validar email\nexport:\n    func valid(email: String) -> bool\ncontracts:\n    - entrada vazia retorna false\n",
            "Use para garantias observáveis da API; export contém assinaturas e rules contém invariantes internas.",
        ),
        (
            "values",
            "Lista alternativas de um enum.",
            "enum Status\npurpose: estado da conta\nvalues:\n    Active\n    Suspended\n",
            "Use nomes ou descrições de alternativas. Campos tipados pertencem a fields.",
        ),
        (
            "notes",
            "Preserva observações adicionais sem confundi-las com finalidade.",
            "class Cache\npurpose: acelerar consultas\nnotes:\n    tamanho ajustado conforme memória disponível\n",
            "Use para contexto auxiliar; obrigações pertencem a rules ou contracts.",
        ),
        (
            "source",
            "Relaciona documentação a arquivos e símbolos de código real.",
            "class UserService\npurpose: gerenciar usuários\nsource:\n    src/users.rs::UserService\n",
            "É metadata documental, sem executar ou exigir o arquivo mapeado; nunca é renomeada como referência Luvyn.",
        ),
    ] {
        row!(
            key,
            "Structure",
            format!("{key}:\n    content"),
            description,
            "símbolo atual (ou bloco aninhado em main)",
            example,
            notes,
            &["purpose", "notes", "self"],
            format!("{key}:\n    ${{1}}")
        );
    }
    row!(
        "export",
        "Relations",
        "export:\n    func name(arg: Type) -> ReturnType",
        "Declara a API pública/documentada; em main também aceita nomes de símbolos.",
        "símbolo atual ou main",
        USERS,
        "Export cria relação export do proprietário à API. Depends registra consumo, não publicação. Métodos exigem func explícito; descriptions são texto indentado oito espaços.",
        &["func", "depends", "main"],
        "export:\n    func ${1:method}(${2:arg}: ${3:String}) -> ${4:()}"
    );
    row!(
        "depends",
        "Relations",
        "depends:\n    Symbol",
        "Registra consumo ou dependência arquitetural explícita.",
        "símbolo atual ou main",
        USERS,
        "Use para componentes necessários ao funcionamento; não para marcar API pública (export) nem implementação de contrato (implements). A assinatura já registra relações uses/returns derivadas.",
        &["export", "implements", "import"],
        "depends:\n    ${1:Symbol}"
    );
    row!(
        "implements",
        "Relations",
        "implements Interface",
        "Liga uma classe ao contrato que ela implementa.",
        "símbolo atual",
        "interface Store\npurpose: persistir dados\n\nclass Database\nimplements Store\npurpose: implementação persistente\n",
        "Use para cumprimento de um contrato; consumo é depends. Não é um alias de depends.",
        &["interface", "depends"],
        "implements ${1:Interface}"
    );
    row!(
        "extends",
        "Relations",
        "extends Base",
        "Registra especialização estrutural de um tipo base.",
        "símbolo atual",
        "class User\npurpose: conta de usuário\n\nclass Admin\nextends User\npurpose: conta com administração\n",
        "Use para herança/especialização documentada; não para dependência transitória. Não simula herança de campos.",
        &["class", "implements"],
        "extends ${1:Base}"
    );
    row!(
        "import",
        "References",
        "import module.Symbol [as Alias] | import module | import \"relative.lyn\"",
        "Desambigua referências; símbolos únicos já resolvem automaticamente.",
        "arquivo / coluna 1",
        "import accounts.User as Account\nimport billing.User as Customer\nclass Invoice\npurpose: faturar cliente\nfields:\n    owner: Account\n    customer: Customer\n",
        "Use quando existem nomes iguais em módulos diferentes ou para explicitar uma fronteira. O exemplo pressupõe accounts.User e billing.User definidos nesses arquivos. Nunca é obrigatório para um nome único no projeto.",
        &["module", "as", "depends"],
        "import ${1:module.Symbol}"
    );
    row!(
        "as",
        "References",
        "import module.Symbol as Alias",
        "Nomeia localmente um import para evitar colisões.",
        "import",
        "import users.User as Account\nclass Session\npurpose: sessão autenticada\nfields:\n    user: Account\n",
        "Use com imports ambíguos; não renomeia o símbolo original. O exemplo pressupõe users.User.",
        &["import"],
        "as ${1:Alias}"
    );
    row!(
        "module",
        "Structure",
        "module qualified.name override",
        "Substitui explicitamente o módulo inferido do caminho do arquivo.",
        "antes de declarações",
        "module users override\nclass User\npurpose: identidade do usuário\n",
        "Normalmente não escreva module: docs/auth/User.lyn infere auth.User sob a source root docs. Symbols no mesmo módulo são visíveis; o nome do projeto não vira namespace automaticamente.",
        &["import", "namespace"],
        "module ${1:name} override"
    );
    row!(
        "namespace",
        "Structure",
        "namespace qualified.name",
        "Define explicitamente um namespace compartilhado entre arquivos.",
        "antes de declarações",
        "namespace users\nclass User\npurpose: identidade do usuário\n",
        "Use para agrupar vários documentos no mesmo módulo lógico. Sem namespace, o módulo vem do caminho; prefira esse default quando suficiente.",
        &["module", "import"],
        "namespace ${1:users}"
    );
    row!(
        "self",
        "References",
        "self.<field>",
        "Referencia field do proprietário atual; self isolado referencia o proprietário.",
        "purpose, rules, behavior, contracts, notes e descrições de métodos",
        "class User\npurpose: identidade do usuário\nfields:\n    name: String\n    email: String\nrules:\n    - self.email deve ser único\nexport:\n    func setName(name: String) -> User\n        self.name = name\n",
        "Não é execução de atribuição. Só fields do proprietário entram em completion; uma função isolada ou main não possui self. Strings e comentários não criam referências.",
        &["fields", "export"],
        "self.${1:field}"
    );
    row!(
        "->",
        "Operators",
        "func name(args) -> ReturnType",
        "Anota o tipo retornado por uma função.",
        "assinaturas",
        "func find<T>(id: i64) -> Option<T>\npurpose: localizar valor pelo identificador\n",
        "A seta é um operador único; > também fecha generics. Option<T> representa ausência; Result<T, Error> representa falha.",
        &["func", "generics"],
        "-> ${1:Type}"
    );
    row!(
        "#",
        "Keywords",
        "# comentário | // comentário",
        "Comentário de linha fora de strings.",
        "qualquer linha",
        "class User # identidade da aplicação\npurpose: identificar usuário\n",
        "Comentários não geram referências ou nodes; use notes para informação que deve entrar no artefato.",
        &["notes"],
        "# ${1:comment}"
    );
    entries.last_mut().unwrap().aliases.push("//".into());
    for key in BUILTINS {
        let example = match *key { "Option" => "type OptionalId = Option<i64>\npurpose: identificador opcional\n".into(), "Result" => "class Error\npurpose: descrever falha\nfunc load() -> Result<String, Error>\npurpose: carregar conteúdo\n".into(), "Vec" => "type IDs = Vec<i64>\npurpose: coleção de identificadores\n".into(), "HashMap" => "type Counts = HashMap<String, i64>\npurpose: contagem por chave\n".into(), _ => format!("class Value\npurpose: armazenar valor\nfields:\n    value: {key}\n") };
        row!(
            *key,
            "Types",
            *key,
            "Tipo documental embutido, sem declaração ou import.",
            "assinaturas e fields",
            example,
            "Não define coerções ou execução. Tipos do projeto e parâmetros genéricos também são aceitos.",
            &["type", "generics"],
            *key
        );
    }
    for key in LITERALS {
        row!(
            *key,
            "Literals",
            *key,
            "Literal booleano em texto documental.",
            "rules, behavior e annotations",
            "class User\npurpose: identidade\nfields:\n    active: bool\nrules:\n    - self.active == true permite acesso\n",
            "Não use literal como tipo; use bool.",
            &["bool", "rules"],
            *key
        );
    }
    entries.sort_by(|a, b| a.category.cmp(&b.category).then(a.keyword.cmp(&b.keyword)));
    entries
});
pub fn dictionary() -> &'static [LanguageEntry] {
    &DICTIONARY
}
pub fn lookup(keyword: &str) -> Option<&'static LanguageEntry> {
    dictionary().iter().find(|e| {
        e.keyword.eq_ignore_ascii_case(keyword)
            || e.aliases.iter().any(|a| a.eq_ignore_ascii_case(keyword))
    })
}
pub fn summary(keyword: Option<&str>) -> crate::Result<String> {
    let entries: Vec<_> = match keyword {
        Some(k) => vec![
            lookup(k)
                .ok_or_else(|| crate::Error::Message(format!("Unknown language entry: {k}")))?,
        ],
        None => dictionary().iter().collect(),
    };
    Ok(format!(
        "Luvyn Language\n\n{}{}",
        if keyword.is_none() {
            "Each .lyn file defines its module relative to the configured source root.\nDocumentation and target projects can be separate: luvyn init --target ../MyApp.\n[project] target selects the project receiving .luvyn/project.lu from luvyn build.\nAndroid client: luvyn ide build --android.\n\n"
        } else {
            ""
        },
        entries
            .iter()
            .map(|e| format!(
                "{}\n  Category: {}\n  {}\n  Contexts: {}{}\n  Notes: {}\n\nExample:\n{}\n",
                e.syntax,
                e.category,
                e.description,
                e.contexts.join(", "),
                if e.aliases.is_empty() {
                    String::new()
                } else {
                    format!("\n  Aliases: {}", e.aliases.join(", "))
                },
                e.notes,
                e.examples.join("\n\n")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
