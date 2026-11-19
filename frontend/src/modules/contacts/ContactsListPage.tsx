import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../../api/client";
import { KIND_LABELS, type ContactListItem, type Page, type Tag } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { Badge, Button, Loading, PageHeader, Pagination, SearchInput } from "../../components/ui";
import { useDebounced, useUrlState } from "../../lib/hooks";
import { Avatar, RoleBadges } from "./common";

// Taille de page de la liste.
const PER_PAGE = 80;

// Liste des contacts : filtres dans l'URL, recherche différée, tri serveur, export CSV.
export function ContactsListPage() {
  const nav = useNavigate();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const [f, setF] = useUrlState({ q: "", kind: "", role: "", tag_id: "", archived: "actifs", sort: "name", page: "1" });
  const [text, setText] = useState(f.q);
  const q = useDebounced(text, 250);

  // Recopie la recherche différée dans l'URL (et revient en page 1).
  useEffect(() => {
    if (q !== f.q) setF({ q, page: "1" });
  }, [q]); // eslint-disable-line react-hooks/exhaustive-deps

  // Paramètres envoyés à l'API = filtres non vides.
  const params = new URLSearchParams({ per_page: String(PER_PAGE) });
  for (const [k, v] of Object.entries(f)) if (v) params.set(k, v);
  const query = params.toString();

  const { data, isLoading } = useQuery({
    queryKey: ["contacts", query],
    queryFn: () => api.get<Page<ContactListItem>>(`/contacts?${query}`),
    placeholderData: (prev) => prev,
  });
  const { data: tags } = useQuery({ queryKey: ["tags"], queryFn: () => api.get<Tag[]>("/tags") });

  // L'export reprend les filtres mais pas la pagination.
  const exportParams = new URLSearchParams(params);
  exportParams.delete("page");
  exportParams.delete("per_page");

  // Colonnes du tableau.
  const columns: Column<ContactListItem>[] = [
    {
      key: "name",
      header: "Nom",
      render: (c) => (
        <span className="name-cell">
          <Avatar name={c.name} kind={c.kind} />
          <span>
            {c.display_name}
            {c.archived_at ? <Badge>Archivé</Badge> : null}
          </span>
        </span>
      ),
    },
    { key: "email", header: "E-mail", render: (c) => c.email },
    { key: "phone", header: "Téléphone", render: (c) => <span className="nowrap">{c.phone || c.mobile}</span> },
    { key: "city", header: "Ville", render: (c) => c.city },
    { key: "country", header: "Pays", render: (c) => c.country },
    { key: "roles", header: "Rôle", render: (c) => <RoleBadges c={c} /> },
    {
      key: "tags",
      header: "Étiquettes",
      render: (c) => (
        <span className="chips">
          {c.tags.map((t) => (
            <span key={t.id} className="chip">
              {t.name}
            </span>
          ))}
        </span>
      ),
    },
  ];

  return (
    <>
      <PageHeader
        title="Contacts"
        actions={
          canEdit ? (
            <Button variant="primary" onClick={() => nav("/contacts/nouveau")}>
              Nouveau
            </Button>
          ) : undefined
        }
      >
        <SearchInput value={text} onChange={setText} placeholder="Nom, société, email, ville, n° client..." />
        {data ? <Pagination page={data.page} perPage={data.per_page} total={data.total} onPage={(p) => setF({ page: String(p) })} /> : null}
      </PageHeader>
      <div className="filter-bar">
        <select aria-label="Type" value={f.kind} onChange={(e) => setF({ kind: e.target.value, page: "1" })}>
          <option value="">Tous les types</option>
          {Object.entries(KIND_LABELS).map(([k, l]) => (
            <option key={k} value={k}>
              {l}
            </option>
          ))}
        </select>
        <select aria-label="Rôle" value={f.role} onChange={(e) => setF({ role: e.target.value, page: "1" })}>
          <option value="">Tous les rôles</option>
          <option value="client">Clients</option>
          <option value="prospect">Prospects</option>
          <option value="stagiaire">Stagiaires</option>
        </select>
        <select aria-label="Étiquette" value={f.tag_id} onChange={(e) => setF({ tag_id: e.target.value, page: "1" })}>
          <option value="">Toutes les étiquettes</option>
          {(tags ?? []).map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
        <select aria-label="Archivage" value={f.archived} onChange={(e) => setF({ archived: e.target.value, page: "1" })}>
          <option value="actifs">Actifs</option>
          <option value="archives">Archivés</option>
          <option value="tous">Tous</option>
        </select>
        {canEdit ? (
          <span className="filter-actions">
            <Button variant="ghost" onClick={() => nav("/contacts/import")}>
              Importer
            </Button>
            <a className="btn btn-ghost" href={`/api/contacts/export?${exportParams}`} download>
              Exporter (CSV)
            </a>
          </span>
        ) : null}
      </div>
      <div className="page-body wide">
        {isLoading || !data ? (
          <Loading />
        ) : (
          <DataTable
            columns={columns}
            rows={data.items}
            rowKey={(c) => c.id}
            onRowClick={(c) => nav(`/contacts/${c.id}`)}
            empty="Aucun contact ne correspond à la recherche"
            serverSort={{ value: f.sort, onChange: (s) => setF({ sort: s, page: "1" }), keys: ["name", "email", "city", "country"] }}
          />
        )}
      </div>
    </>
  );
}
