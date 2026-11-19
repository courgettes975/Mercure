import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import { PROJECT_STATUS, type Page, type Person, type Project, type ProjectStatus, type TimesheetRow } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { MoneyField, StatusBadge } from "../../components/Fields";
import { AsyncSelect, Option } from "../../components/Pickers";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Card, Checkbox, ConfirmDialog, Loading, Modal, PageHeader, Pagination, SearchInput, SelectField, TextAreaField, TextField } from "../../components/ui";
import { formatDate, formatMinutes, formatMoney } from "../../lib/format";
import { useDebounced, useUrlState } from "../../lib/hooks";
import { searchContacts } from "../../lib/search";

interface ProjectForm {
  name: string;
  client_id: number | null;
  description: string;
  start_date: string;
  end_date: string;
  status: ProjectStatus;
  billable: boolean;
  hourly_rate: number;
  member_ids: number[];
}

const EMPTY: ProjectForm = { name: "", client_id: null, description: "", start_date: "", end_date: "", status: "EN_COURS", billable: true, hourly_rate: 0, member_ids: [] };

// Projet serveur -> formulaire éditable.
function toForm(p: Project): ProjectForm {
  return {
    name: p.name,
    client_id: p.client_id,
    description: p.description,
    start_date: p.start_date ?? "",
    end_date: p.end_date ?? "",
    status: p.status,
    billable: p.billable,
    hourly_rate: p.hourly_rate,
    member_ids: (p.members ?? []).map((m) => m.id),
  };
}

// Champs communs à la création et à la fiche (nom, client, dates, statut, tarif, intervenants).
function ProjectFormFields({ form, setForm, client, setClient, disabled }: {
  form: ProjectForm;
  setForm: (f: ProjectForm) => void;
  client: Option | null;
  setClient: (o: Option | null) => void;
  disabled?: boolean;
}) {
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people") });
  return (
    <fieldset disabled={disabled} className="plain form-stack">
      <div className="grid-2">
        <TextField label="Nom du projet" required value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} wide />
        <AsyncSelect label="Client" value={client} search={searchContacts} disabled={disabled} onChange={(o) => { setClient(o); setForm({ ...form, client_id: o?.id ?? null }); }} />
        <SelectField label="Statut" value={form.status} onChange={(e) => setForm({ ...form, status: e.target.value as ProjectStatus })}
          options={Object.entries(PROJECT_STATUS).map(([value, s]) => ({ value, label: s.label }))} />
        <TextField label="Date de début" type="date" value={form.start_date} onChange={(e) => setForm({ ...form, start_date: e.target.value })} />
        <TextField label="Date de fin" type="date" value={form.end_date} onChange={(e) => setForm({ ...form, end_date: e.target.value })} />
        <MoneyField label="Taux horaire" value={form.hourly_rate} onChange={(v) => setForm({ ...form, hourly_rate: v })} disabled={disabled} />
        <div className="field">
          <label>Facturation</label>
          <Checkbox label="Temps facturables" checked={form.billable} onChange={(e) => setForm({ ...form, billable: e.target.checked })} />
        </div>
        <TextAreaField label="Description" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
      </div>
      <div className="field">
        <label>Intervenants</label>
        <div className="role-row">
          {(people ?? []).map((p) => (
            <Checkbox key={p.id} label={p.display_name} checked={form.member_ids.includes(p.id)}
              onChange={(e) => setForm({ ...form, member_ids: e.target.checked ? [...form.member_ids, p.id] : form.member_ids.filter((x) => x !== p.id) })} />
          ))}
        </div>
      </div>
    </fieldset>
  );
}

// Formulaire -> corps JSON attendu par l'API.
function payload(f: ProjectForm) {
  return { ...f, start_date: f.start_date || null, end_date: f.end_date || null };
}

// Liste des projets (hors missions) avec filtres dans l'URL et fenêtre de création.
export function ProjectsListPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const [f, setF] = useUrlState({ q: "", status: "", kind: "PROJET", archived: "actifs", page: "1" });
  const [text, setText] = useState(f.q);
  const q = useDebounced(text, 250);
  useEffect(() => {
    if (q !== f.q) setF({ q, page: "1" });
  }, [q]); // eslint-disable-line react-hooks/exhaustive-deps
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v) params.set(k, v);
  const { data, isLoading } = useQuery({ queryKey: ["projects", params.toString()], queryFn: () => api.get<Page<Project>>(`/projects?${params}`), placeholderData: (p) => p });

  const [open, setOpen] = useState(false);
  const [form, setForm] = useState<ProjectForm>(EMPTY);
  const [client, setClient] = useState<Option | null>(null);
  const [error, setError] = useState("");
  // Création puis ouverture de la fiche.
  const create = useMutation({
    mutationFn: () => api.post<Project>("/projects", payload(form)),
    onSuccess: (p) => {
      qc.invalidateQueries({ queryKey: ["projects"] });
      toast("Projet créé");
      nav(`/projets/${p.id}`);
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });

  const columns: Column<Project>[] = [
    { key: "ref", header: "Référence", width: "130px", render: (p) => p.reference },
    { key: "name", header: "Projet", render: (p) => <>{p.name} {p.kind === "FORMATION" ? <Badge tone="info">Formation</Badge> : null}</> },
    { key: "client", header: "Client", render: (p) => p.client_name },
    { key: "dates", header: "Période", render: (p) => [formatDate(p.start_date), formatDate(p.end_date)].filter(Boolean).join(" - ") },
    { key: "time", header: "Temps", align: "right", render: (p) => formatMinutes(p.minutes_total) },
    { key: "todo", header: "À facturer", align: "right", render: (p) => (p.amount_uninvoiced ? formatMoney(p.amount_uninvoiced) : "") },
    { key: "status", header: "Statut", render: (p) => <StatusBadge map={PROJECT_STATUS} value={p.status} /> },
  ];

  return (
    <>
      <PageHeader title="Projets" actions={canEdit ? <Button variant="primary" onClick={() => { setForm(EMPTY); setClient(null); setError(""); setOpen(true); }}>Nouveau</Button> : undefined}>
        <SearchInput value={text} onChange={setText} placeholder="Nom, référence, client..." />
        {data ? <Pagination page={data.page} perPage={data.per_page} total={data.total} onPage={(p) => setF({ page: String(p) })} /> : null}
      </PageHeader>
      <div className="filter-bar">
        <select aria-label="Type" value={f.kind} onChange={(e) => setF({ kind: e.target.value, page: "1" })}>
          <option value="">Projets et formations</option>
          <option value="PROJET">Projets</option>
          <option value="FORMATION">Missions de formation</option>
        </select>
        <select aria-label="Statut" value={f.status} onChange={(e) => setF({ status: e.target.value, page: "1" })}>
          <option value="">Tous les statuts</option>
          {Object.entries(PROJECT_STATUS).map(([k, s]) => <option key={k} value={k}>{s.label}</option>)}
        </select>
        <select aria-label="Archivage" value={f.archived} onChange={(e) => setF({ archived: e.target.value, page: "1" })}>
          <option value="actifs">Actifs</option>
          <option value="archives">Archivés</option>
          <option value="tous">Tous</option>
        </select>
      </div>
      <div className="page-body wide">
        {isLoading || !data ? <Loading /> : (
          <DataTable columns={columns} rows={data.items} rowKey={(p) => p.id} empty="Aucun projet"
            onRowClick={(p) => nav(p.mission_id ? `/missions/${p.mission_id}` : `/projets/${p.id}`)} />
        )}
      </div>
      <Modal open={open} title="Nouveau projet" onClose={() => setOpen(false)}
        footer={<><Button onClick={() => setOpen(false)}>Annuler</Button><Button variant="primary" loading={create.isPending} onClick={() => create.mutate()}>Créer</Button></>}>
        <div className="form-stack">
          <Alert>{error}</Alert>
          <ProjectFormFields form={form} setForm={setForm} client={client} setClient={setClient} />
        </div>
      </Modal>
    </>
  );
}

// Fiche projet : édition, tâches, archivage, suppression ; les missions renvoient vers leur propre fiche.
export function ProjectPage() {
  const { id } = useParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const { data } = useQuery({ queryKey: ["project", id], queryFn: () => api.get<Project>(`/projects/${id}`) });
  const { data: times } = useQuery({ queryKey: ["timesheets", `project=${id}`], queryFn: () => api.get<{ items: TimesheetRow[] }>(`/timesheets?project_id=${id}&limit=500`) });
  const [form, setForm] = useState<ProjectForm | null>(null);
  const [client, setClient] = useState<Option | null>(null);
  const [error, setError] = useState("");
  const [taskName, setTaskName] = useState("");
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    if (data) {
      if (data.mission_id) nav(`/missions/${data.mission_id}`, { replace: true });
      setForm(toForm(data));
      setClient({ id: data.client_id, label: data.client_name });
    }
  }, [data, nav]);

  const onError = (e: unknown) => setError(e instanceof ApiError ? e.message : "Erreur inattendue");
  const refresh = (p: Project, msg: string) => {
    qc.setQueryData(["project", id], p);
    qc.invalidateQueries({ queryKey: ["projects"] });
    setError("");
    toast(msg);
  };
  // Enregistrement / archivage / suppression.
  const save = useMutation({ mutationFn: () => api.put<Project>(`/projects/${id}`, payload(form!)), onSuccess: (p) => refresh(p, "Projet enregistré"), onError });
  const archive = useMutation({ mutationFn: (a: boolean) => api.post<Project>(`/projects/${id}/archive`, { archived: a }), onSuccess: (p) => refresh(p, p.archived_at ? "Projet archivé" : "Projet désarchivé"), onError });
  const remove = useMutation({ mutationFn: () => api.del(`/projects/${id}`), onSuccess: () => { qc.invalidateQueries({ queryKey: ["projects"] }); nav("/projets"); }, onError: (e) => { setConfirm(false); onError(e); } });
  // Gestion des tâches (ajout, activation).
  const addTask = useMutation({
    mutationFn: () => api.post(`/projects/${id}/tasks`, { name: taskName }),
    onSuccess: () => { setTaskName(""); qc.invalidateQueries({ queryKey: ["project", id] }); },
    onError,
  });
  const toggleTask = useMutation({
    mutationFn: (t: { id: number; active: boolean }) => api.patch(`/tasks/${t.id}`, { active: !t.active }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["project", id] }),
    onError,
  });

  if (!data || !form) return <Loading />;
  const dirty = JSON.stringify(toForm(data)) !== JSON.stringify(form);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate();
  };

  return (
    <form onSubmit={submit}>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb"><Link to="/projets">Projets</Link><span>{data.name}</span></div>
          <StatusBadge map={PROJECT_STATUS} value={data.status} />
          {canEdit && dirty ? (
            <>
              <Button variant="primary" type="submit" loading={save.isPending}>Enregistrer</Button>
              <Button type="button" onClick={() => setForm(toForm(data))}>Annuler</Button>
            </>
          ) : null}
        </div>
        {canEdit ? (
          <div className="page-header-right">
            {data.amount_uninvoiced > 0 ? <Button type="button" variant="primary" onClick={() => nav(`/facturation/depuis-projet/${data.id}`)}>Facturer ({formatMoney(data.amount_uninvoiced)})</Button> : null}
            <Button type="button" variant="ghost" onClick={() => archive.mutate(!data.archived_at)}>{data.archived_at ? "Désarchiver" : "Archiver"}</Button>
            <Button type="button" variant="ghost" className="text-danger" onClick={() => setConfirm(true)}>Supprimer</Button>
          </div>
        ) : null}
      </div>
      <div className="page-body wide">
        <Alert>{error}</Alert>
        <div className="stats">
          <div className="stat"><span className="stat-label">Référence</span><span className="stat-value small-value">{data.reference}</span></div>
          <div className="stat"><span className="stat-label">Temps passé</span><span className="stat-value">{formatMinutes(data.minutes_total)}</span></div>
          <div className="stat"><span className="stat-label">Temps facturable</span><span className="stat-value">{formatMinutes(data.minutes_billable)}</span></div>
          <div className="stat"><span className="stat-label">Reste à facturer</span><span className="stat-value">{formatMoney(data.amount_uninvoiced)}</span></div>
        </div>
        <div className="grid-2 cards">
          <Card title="Projet">
            <ProjectFormFields form={form} setForm={setForm} client={client} setClient={setClient} disabled={!canEdit} />
          </Card>
          <Card title="Tâches">
            {canEdit ? (
              <div className="inline-form">
                <TextField label="Nouvelle tâche" value={taskName} onChange={(e) => setTaskName(e.target.value)} />
                <Button type="button" variant="primary" disabled={!taskName.trim()} onClick={() => addTask.mutate()}>Ajouter</Button>
              </div>
            ) : null}
            <ul className="plain-list">
              {(data.tasks ?? []).map((t) => (
                <li key={t.id}>
                  <span className={t.active ? "" : "muted"}>{t.name}</span>
                  {canEdit ? <Button type="button" variant="ghost" onClick={() => toggleTask.mutate(t)}>{t.active ? "Archiver" : "Réactiver"}</Button> : null}
                </li>
              ))}
              {(data.tasks ?? []).length === 0 ? <li className="muted">Aucune tâche</li> : null}
            </ul>
          </Card>
        </div>
        <Card title="Temps enregistrés">
          <DataTable
            columns={[
              { key: "date", header: "Date", render: (t: TimesheetRow) => formatDate(t.date) },
              { key: "user", header: "Intervenant", render: (t: TimesheetRow) => t.user_name },
              { key: "task", header: "Tâche", render: (t: TimesheetRow) => t.task_name ?? "" },
              { key: "desc", header: "Description", render: (t: TimesheetRow) => t.description },
              { key: "dur", header: "Durée", align: "right", render: (t: TimesheetRow) => formatMinutes(t.duration_minutes) },
              { key: "amt", header: "Montant", align: "right", render: (t: TimesheetRow) => (t.billable ? formatMoney(t.amount) : <span className="muted">non facturable</span>) },
              { key: "inv", header: "Facture", render: (t: TimesheetRow) => (t.invoice_id ? <Link to={`/facturation/${t.invoice_id}`}>{t.invoice_number ?? "Brouillon"}</Link> : "") },
            ]}
            rows={times?.items ?? []}
            rowKey={(t) => t.id}
            empty="Aucun temps"
          />
        </Card>
      </div>
      <ConfirmDialog open={confirm} title="Supprimer le projet" message={`Supprimer « ${data.name} » ? Possible uniquement s'il n'a ni temps ni facture.`} confirmLabel="Supprimer" danger
        loading={remove.isPending} onConfirm={() => remove.mutate()} onCancel={() => setConfirm(false)} />
    </form>
  );
}
