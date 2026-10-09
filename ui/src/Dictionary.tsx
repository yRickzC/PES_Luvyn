import { useEffect, useState } from "react";
import { api } from "./api";
export type LanguageEntry = {
  keyword: string;
  completion: string;
  category: string;
  syntax: string;
  description: string;
  contexts: string[];
  examples: string[];
  notes: string;
  aliases: string[];
  related: string[];
};
export function useDictionary() {
  const [entries, setEntries] = useState<LanguageEntry[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    api("language")
      .then((data) => setEntries(data.entries))
      .catch((e) => setError(String(e)));
  }, []);
  return { entries, error };
}
export function LanguageSidebar({
  entries,
  selected,
  select,
  error,
}: {
  entries: LanguageEntry[];
  selected: string;
  select: (key: string) => void;
  error: string;
}) {
  const [query, setQuery] = useState("");
  const filtered = entries.filter((e) =>
    `${e.keyword} ${e.description} ${e.aliases.join(" ")}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  return (
    <div className="language-sidebar">
      <input
        aria-label="Pesquisar linguagem"
        placeholder="Pesquisar linguagem…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {error && <p>{error}</p>}
      {[...new Set(filtered.map((e) => e.category))].map((category) => (
        <section key={category}>
          <h4>{category}</h4>
          {filtered
            .filter((e) => e.category === category)
            .map((e) => (
              <button
                key={e.keyword}
                className={selected === e.keyword ? "active" : ""}
                onClick={() => select(e.keyword)}
              >
                {e.keyword}
                {e.aliases.length > 0 && <small>{e.aliases.join(", ")}</small>}
              </button>
            ))}
        </section>
      ))}
    </div>
  );
}
export function LanguageDetails({
  entry,
  select,
}: {
  entry?: LanguageEntry;
  select: (key: string) => void;
}) {
  if (!entry)
    return (
      <article className="language-details">
        <h1>Language</h1>
        <p>Selecione uma entrada do dicionário oficial do Core.</p>
      </article>
    );
  return (
    <article className="language-details">
      <small>{entry.category}</small>
      <h1>{entry.keyword}</h1>
      <p>{entry.description}</p>
      <h3>Sintaxe</h3>
      <pre>{entry.syntax}</pre>
      <h3>Contextos permitidos</h3>
      <ul>
        {entry.contexts.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <h3>Exemplos</h3>
      {entry.examples.map((e, i) => (
        <pre key={i}>{e}</pre>
      ))}
      {entry.notes && <p>{entry.notes}</p>}
      {entry.aliases.length > 0 && <p>Aliases: {entry.aliases.join(", ")}</p>}
      <h3>Relacionados</h3>
      <div className="language-related">
        {entry.related.map((k) => (
          <button key={k} onClick={() => select(k)}>
            {k}
          </button>
        ))}
      </div>
    </article>
  );
}
