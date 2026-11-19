import { api } from "../api/client";
import type { ContactListItem, Page } from "../api/types";
import type { Option } from "../components/Pickers";

/** Recherche de contacts pour les sélecteurs (clients, contacts, stagiaires). */
export async function searchContacts(q: string, opts: { companiesOnly?: boolean; persons?: boolean } = {}): Promise<Option[]> {
  const p = new URLSearchParams({ per_page: "20", q });
  if (opts.companiesOnly) p.set("companies_only", "true");
  if (opts.persons) p.set("kind", "PARTICULIER");
  const page = await api.get<Page<ContactListItem>>(`/contacts?${p}`);
  return page.items.map((c) => ({ id: c.id, label: c.display_name }));
}
