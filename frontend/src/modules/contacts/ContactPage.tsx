import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import {
  KIND_LABELS,
  type ContactDetail,
  type ContactInput,
  type ContactKind,
  type ContactListItem,
  type HistoryItem,
  type NoteItem,
  type Page,
  type Tag,
} from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { AsyncSelect, Option, TagPicker } from "../../components/Pickers";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Card, Checkbox, ConfirmDialog, Loading, TextAreaField, TextField } from "../../components/ui";
import { formatDateTime, formatMoney } from "../../lib/format";
import { Avatar, describeChanges } from "./common";

const EMPTY: ContactInput = {
  kind: "PARTICULIER",
  parent_id: null,
  name: "",
  first_name: "",
  is_client: false,
  is_prospect: false,
  is_stagiaire: false,
  job_title: "",
  email: "",
  phone: "",
  mobile: "",
  street: "",
  street2: "",
  zip: "",
  city: "",
  country: "France",
  website: "",
  notes: "",
  siren: "",
  siret: "",
  vat_number: "",
  duns: "",
  tag_ids: [],
};

// Fiche serveur -> formulaire éditable.
function toInput(d: ContactDetail): ContactInput {
  const out = { ...EMPTY };
  for (const k of Object.keys(EMPTY) as (keyof ContactInput)[]) {
    if (k !== "tag_ids") (out as Record<string, unknown>)[k] = d[k as keyof ContactDetail];
  }
  out.tag_ids = d.tags.map((t) => t.id);
  return out;
}

type Tab = "associes" | "notes" | "activite";

// Fiche contact : création (/contacts/nouveau) ou édition, avec onglets Activité / Contacts associés.
export function ContactPage() {
  const { id } = useParams();
  const isNew = id === "nouveau";
  const [search] = useSearchParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");

  // Chargement de la fiche (désactivé en création).
  const { data, isLoading } = useQuery({
    queryKey: ["contact", id],
    queryFn: () => api.get<ContactDetail>(`/contacts/${id}`),
    enabled: !isNew,
  });
  const { data: tags } = useQuery({ queryKey: ["tags"], queryFn: () => api.get<Tag[]>("/tags") });

  const [form, setForm] = useState<ContactInput | null>(null);
  const [parent, setParent] = useState<Option | null>(null);
  const [error, setError] = useState("");
  const [tab, setTab] = useState<Tab>("activite");
  const [confirm, setConfirm] = useState<"delete" | null>(null);

  // Initialisation du formulaire : vide (avec société pré-remplie depuis l'URL) ou depuis la fiche chargée.
  useEffect(() => {
    if (isNew) {
      const pid = search.get("parent_id");
      setForm({ ...EMPTY, parent_id: pid ? Number(pid) : null, kind: (search.get("kind") as ContactKind) || "PARTICULIER" });
      setParent(pid ? { id: Number(pid), label: search.get("parent_name") ?? "" } : null);
    } else if (data) {
      setForm(toInput(data));
      setParent(data.parent_id ? { id: data.parent_id, label: data.parent_name ?? "" } : null);
      setTab(data.kind === "PARTICULIER" ? "activite" : "associes");
    }
  }, [data, isNew, search]);

  // Recherche des sociétés de rattachement, en excluant le contact lui-même.
  const searchCompanies = useCallback(
    (q: string) =>
      api
        .get<Page<ContactListItem>>(`/contacts?companies_only=true&per_page=20&q=${encodeURIComponent(q)}`)
        .then((p) => p.items.filter((c) => c.id !== data?.id).map((c) => ({ id: c.id, label: c.display_name }))),
    [data?.id],
  );

  // Après enregistrement : mise à jour du cache, rafraîchissement des listes, notification.
  const onSaved = (d: ContactDetail, msg: string) => {
    qc.setQueryData(["contact", String(d.id)], d);
    qc.invalidateQueries({ queryKey: ["contacts"] });
    qc.invalidateQueries({ queryKey: ["contact-activity", String(d.id)] });
    setError("");
    toast(msg);
  };
  const onError = (e: unknown) => {
    setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    window.scrollTo({ top: 0, behavior: "smooth" });
  };

  // Création ou modification.
  const save = useMutation({
    mutationFn: (f: ContactInput) => (isNew ? api.post<ContactDetail>("/contacts", f) : api.put<ContactDetail>(`/contacts/${id}`, f)),
    onSuccess: (d) => {
      onSaved(d, isNew ? "Contact créé" : "Contact enregistré");
      if (isNew) nav(`/contacts/${d.id}`, { replace: true });
    },
    onError,
  });
  // Archiver / désarchiver / dupliquer.
  const action = useMutation({
    mutationFn: (a: "archive" | "unarchive" | "duplicate") => api.post<ContactDetail>(`/contacts/${id}/${a}`),
    onSuccess: (d, a) => {
      onSaved(d, a === "duplicate" ? "Copie créée" : a === "archive" ? "Contact archivé" : "Contact désarchivé");
      if (a === "duplicate") nav(`/contacts/${d.id}`);
    },
    onError,
  });
  // Suppression (après confirmation).
  const remove = useMutation({
    mutationFn: () => api.del(`/contacts/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["contacts"] });
      toast("Contact supprimé");
      nav("/contacts");
    },
    onError: (e) => {
      setConfirm(null);
      onError(e);
    },
  });
  const createTag = async (name: string) => {
    const t = await api.post<Tag>("/tags", { name });
    qc.invalidateQueries({ queryKey: ["tags"] });
    return { id: t.id, label: t.name };
  };

  if ((!isNew && isLoading) || !form) return <Loading />;

  const set = <K extends keyof ContactInput>(k: K, v: ContactInput[K]) => setForm({ ...form, [k]: v });
  const txt = (k: keyof ContactInput) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => set(k, e.target.value as never);
  const person = form.kind === "PARTICULIER";
  const dirty = isNew || (data && JSON.stringify(toInput(data)) !== JSON.stringify(form));
  const title = isNew ? "Nouveau contact" : data?.display_name ?? "";

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate(form);
  };

  return (
    <form onSubmit={submit}>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb">
            <Link to="/contacts">Contacts</Link>
            <span>{title}</span>
          </div>
          {canEdit && dirty ? (
            <>
              <Button variant="primary" type="submit" loading={save.isPending}>
                Enregistrer
              </Button>
              <Button type="button" onClick={() => (isNew ? nav("/contacts") : data && setForm(toInput(data)))}>
                Annuler
              </Button>
            </>
          ) : null}
        </div>
        {!isNew && canEdit && data ? (
          <div className="page-header-right">
            <Button type="button" variant="ghost" onClick={() => action.mutate("duplicate")}>
              Dupliquer
            </Button>
            <Button type="button" variant="ghost" onClick={() => action.mutate(data.archived_at ? "unarchive" : "archive")}>
              {data.archived_at ? "Désarchiver" : "Archiver"}
            </Button>
            <Button type="button" variant="ghost" className="text-danger" onClick={() => setConfirm("delete")}>
              Supprimer
            </Button>
          </div>
        ) : null}
      </div>

      <div className="page-body wide">
        <Alert>{error}</Alert>
        {data?.archived_at ? <Alert tone="info">Ce contact est archivé : il n'apparaît plus dans les listes et les recherches par défaut.</Alert> : null}

        <fieldset disabled={!canEdit} className="plain form-stack">
          <Card>
            <div className="contact-head">
              <Avatar name={form.name || "?"} kind={form.kind} size={72} />
              <div className="contact-head-main">
                <div className="segmented" role="radiogroup" aria-label="Type de contact">
                  {(Object.keys(KIND_LABELS) as ContactKind[]).map((k) => (
                    <label key={k} className={form.kind === k ? "active" : ""}>
                      <input
                        type="radio"
                        name="kind"
                        checked={form.kind === k}
                        onChange={() => setForm({ ...form, kind: k, parent_id: k === "PARTICULIER" ? form.parent_id : null })}
                      />
                      {KIND_LABELS[k]}
                    </label>
                  ))}
                </div>
                <div className="grid-2">
                  <TextField label={person ? "Nom" : "Raison sociale"} required value={form.name} onChange={txt("name")} className="input-lg" autoFocus={isNew} />
                  {person ? <TextField label="Prénom" value={form.first_name} onChange={txt("first_name")} className="input-lg" /> : <span />}
                  {person ? (
                    <AsyncSelect
                      label="Société / organisme"
                      value={parent}
                      search={searchCompanies}
                      placeholder="Rechercher une société..."
                      disabled={!canEdit}
                      onChange={(o) => {
                        setParent(o);
                        set("parent_id", o?.id ?? null);
                      }}
                    />
                  ) : null}
                  {person ? <TextField label="Fonction" value={form.job_title} onChange={txt("job_title")} placeholder="ex. Responsable formation" /> : null}
                </div>
                <div className="role-row">
                  <Checkbox label="Client" checked={form.is_client} onChange={(e) => setForm({ ...form, is_client: e.target.checked, is_prospect: e.target.checked ? false : form.is_prospect })} />
                  <Checkbox label="Prospect" checked={form.is_prospect} onChange={(e) => setForm({ ...form, is_prospect: e.target.checked, is_client: e.target.checked ? false : form.is_client })} />
                  <Checkbox label="Stagiaire" checked={form.is_stagiaire} onChange={(e) => set("is_stagiaire", e.target.checked)} />
                  {data?.customer_number ? <Badge tone="neutral">N° client {data.customer_number}</Badge> : null}
                </div>
              </div>
            </div>
          </Card>

          <div className="grid-2 cards">
            <Card title="Coordonnées">
              <div className="grid-2">
                <TextField label="Email" type="email" value={form.email} onChange={txt("email")} />
                <TextField label="Téléphone" type="tel" value={form.phone} onChange={txt("phone")} />
                <TextField label="Mobile" type="tel" value={form.mobile} onChange={txt("mobile")} />
                <TextField label="Site web" value={form.website} onChange={txt("website")} placeholder="https://" />
              </div>
              <TagPicker
                label="Étiquettes"
                all={(tags ?? []).map((t) => ({ id: t.id, label: t.name }))}
                selected={form.tag_ids}
                onChange={(ids) => set("tag_ids", ids)}
                onCreate={canEdit ? createTag : undefined}
                disabled={!canEdit}
              />
            </Card>
            <Card title="Adresse">
              <div className="grid-2">
                <TextField label="Adresse" value={form.street} onChange={txt("street")} wide />
                <TextField label="Complément" value={form.street2} onChange={txt("street2")} wide />
                <TextField label="Code postal" value={form.zip} onChange={txt("zip")} />
                <TextField label="Ville" value={form.city} onChange={txt("city")} />
                <TextField label="Pays" value={form.country} onChange={txt("country")} />
              </div>
            </Card>
          </div>

          <Card title="Identifiants légaux">
            <div className="grid-4">
              <TextField label="SIREN" value={form.siren} onChange={txt("siren")} inputMode="numeric" />
              <TextField label="SIRET" value={form.siret} onChange={txt("siret")} inputMode="numeric" />
              <TextField label="N° TVA intracommunautaire" value={form.vat_number} onChange={txt("vat_number")} />
              <TextField label="DUNS" value={form.duns} onChange={txt("duns")} inputMode="numeric" />
            </div>
          </Card>
        </fieldset>

        {!isNew && data ? <ContactActivity id={data.id} canSeeInvoices={canEdit} /> : null}

        {!isNew && data ? (
          <Card>
            <nav className="tabs inner" aria-label="Sections du contact">
              {!person || data.children_count > 0 ? (
                <button type="button" className={`tab ${tab === "associes" ? "active" : ""}`} onClick={() => setTab("associes")}>
                  Contacts associés ({data.children_count})
                </button>
              ) : null}
              <button type="button" className={`tab ${tab === "notes" ? "active" : ""}`} onClick={() => setTab("notes")}>
                Notes internes
              </button>
              <button type="button" className={`tab ${tab === "activite" ? "active" : ""}`} onClick={() => setTab("activite")}>
                Activité et historique
              </button>
            </nav>
            {tab === "associes" ? <ChildrenTab parent={data} canEdit={canEdit} /> : null}
            {tab === "notes" ? (
              <fieldset disabled={!canEdit} className="plain">
                <TextAreaField label="Notes internes (non imprimées)" rows={6} value={form.notes} onChange={txt("notes")} />
              </fieldset>
            ) : null}
            {tab === "activite" ? <ActivityTab id={data.id} canEdit={canEdit} /> : null}
          </Card>
        ) : null}
      </div>

      <ConfirmDialog
        open={confirm === "delete"}
        title="Supprimer le contact"
        message={`Supprimer définitivement « ${data?.display_name} » ? Cette action est irréversible. Si le contact a un historique, préférez l'archivage.`}
        confirmLabel="Supprimer"
        danger
        loading={remove.isPending}
        onConfirm={() => remove.mutate()}
        onCancel={() => setConfirm(null)}
      />
    </form>
  );
}

// Onglet "Contacts associés" d'une société : personnes rattachées + bouton d'ajout.
function ChildrenTab({ parent, canEdit }: { parent: ContactDetail; canEdit: boolean }) {
  const nav = useNavigate();
  const { data } = useQuery({
    queryKey: ["contacts", `parent=${parent.id}`],
    queryFn: () => api.get<Page<ContactListItem>>(`/contacts?parent_id=${parent.id}&archived=tous&per_page=500`),
  });
  const columns: Column<ContactListItem>[] = [
    { key: "name", header: "Nom", render: (c) => `${c.name} ${c.first_name}`.trim(), sortValue: (c) => c.name },
    { key: "email", header: "Email", render: (c) => c.email },
    { key: "phone", header: "Téléphone", render: (c) => c.phone || c.mobile },
    { key: "st", header: "", render: (c) => (c.archived_at ? <Badge>Archivé</Badge> : null) },
  ];
  return (
    <div className="form-stack">
      {canEdit ? (
        <div>
          <Button
            type="button"
            onClick={() => nav(`/contacts/nouveau?parent_id=${parent.id}&parent_name=${encodeURIComponent(parent.display_name)}`)}
          >
            Ajouter un interlocuteur
          </Button>
        </div>
      ) : null}
      <DataTable columns={columns} rows={data?.items ?? []} rowKey={(c) => c.id} onRowClick={(c) => nav(`/contacts/${c.id}`)} empty="Aucun interlocuteur" />
    </div>
  );
}

// Onglet "Activité" : notes libres + historique du journal.
function ActivityTab({ id, canEdit }: { id: number; canEdit: boolean }) {
  const qc = useQueryClient();
  const [body, setBody] = useState("");
  const { data } = useQuery({
    queryKey: ["contact-activity", String(id)],
    queryFn: () => api.get<{ notes: NoteItem[]; history: HistoryItem[] }>(`/contacts/${id}/activity`),
  });
  const add = useMutation({
    mutationFn: () => api.post<NoteItem>(`/contacts/${id}/notes`, { body }),
    onSuccess: () => {
      setBody("");
      qc.invalidateQueries({ queryKey: ["contact-activity", String(id)] });
    },
  });

  type Ev = { key: string; date: string; who: string; title: string; lines: string[]; note?: boolean };
  const events: Ev[] = [
    ...(data?.notes ?? []).map((n) => ({ key: `n${n.id}`, date: n.created_at, who: n.author_name ?? "", title: "Note", lines: [n.body], note: true })),
    ...(data?.history ?? []).map((h) => ({
      key: `h${h.id}`,
      date: h.created_at,
      who: h.user_name ?? "Système",
      title: { CREATION: "Contact créé", MODIFICATION: "Modification", ARCHIVAGE: "Archivé", DESARCHIVAGE: "Désarchivé" }[h.action] ?? h.action,
      lines: h.action === "CREATION" ? [] : describeChanges(h.changes),
    })),
  ].sort((a, b) => b.date.localeCompare(a.date));

  return (
    <div className="form-stack">
      {canEdit ? (
        <div className="note-box">
          <textarea aria-label="Nouvelle note" rows={2} placeholder="Ajouter une note (appel, échange, information...)" value={body} onChange={(e) => setBody(e.target.value)} />
          <Button type="button" variant="primary" disabled={!body.trim()} loading={add.isPending} onClick={() => add.mutate()}>
            Ajouter la note
          </Button>
        </div>
      ) : null}
      <ul className="timeline">
        {events.map((e) => (
          <li key={e.key} className={e.note ? "note" : ""}>
            <div className="timeline-head">
              <strong>{e.who}</strong> <span className="muted small">{formatDateTime(e.date)}</span> <span className="muted small">{e.title}</span>
            </div>
            {e.lines.map((l, i) => (
              <div key={i} className={e.note ? "note-body" : "small"}>
                {l}
              </div>
            ))}
          </li>
        ))}
        {events.length === 0 ? <li className="muted">Aucune activité</li> : null}
      </ul>
    </div>
  );
}

// Compteurs de missions et factures du contact avec liens vers les listes filtrées.
function ContactActivity({ id, canSeeInvoices }: { id: number; canSeeInvoices: boolean }) {
  const nav = useNavigate();
  const { data: missions } = useQuery({ queryKey: ["missions", `client=${id}`], queryFn: () => api.get<{ total: number }>(`/missions?client_id=${id}&per_page=1&archived=tous`) });
  const { data: invoices } = useQuery({
    queryKey: ["invoices", `partner=${id}`],
    queryFn: () => api.get<{ total: number; sum_ttc: number; sum_due: number }>(`/invoices?partner_id=${id}&per_page=1`),
    enabled: canSeeInvoices,
  });
  return (
    <div className="stats">
      <button type="button" className="stat clickable" onClick={() => nav(`/missions?client_id=${id}`)}>
        <span className="stat-label">Missions (client)</span><span className="stat-value">{missions?.total ?? "..."}</span>
      </button>
      {canSeeInvoices ? (
        <button type="button" className="stat clickable" onClick={() => nav(`/facturation?partner_id=${id}`)}>
          <span className="stat-label">Facturé TTC</span><span className="stat-value">{invoices ? formatMoney(invoices.sum_ttc) : "..."}</span>
          <span className="stat-sub">{invoices?.total ?? 0} facture(s), reste dû {invoices ? formatMoney(invoices.sum_due) : ""}</span>
        </button>
      ) : null}
    </div>
  );
}
