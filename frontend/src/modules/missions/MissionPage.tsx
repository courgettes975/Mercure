import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import {
  DOC_LABELS,
  EXPENSE_LABELS,
  FORMAT_LABELS,
  INVOICE_STATUS,
  MISSION_STATUS,
  MISSION_STATUS_ORDER,
  MODALITY_LABELS,
  PRICING_LABELS,
  type Expense,
  type MissionDetail,
  type MissionInput,
  type MissionStatus,
  type Participant,
  type Person,
  type Session,
  type Product,
  type Software,
  type TrainingCenter,
} from "../../api/types";

/** Options du menu Centre : la liste de référence, plus le centre actuel s'il en a été retiré. */
// Options du sélecteur de produit (tarif) et application au formulaire d'une mission :
// unité "jour" -> tarif journalier, sinon tarif horaire.
export function productOptions(products: Product[]) {
  return [{ value: "", label: "- choisir un produit -" }, ...products.filter((p) => p.active).map((p) => ({ value: String(p.id), label: `${p.name} (${formatMoney(p.unit_price)}/${p.unit})` }))];
}
export function applyProductRate<T extends { pricing_mode: string; hourly_rate: number; daily_rate: number }>(form: T, products: Product[], id: string): T {
  const p = products.find((x) => String(x.id) === id);
  if (!p) return form;
  return p.unit.toLowerCase().startsWith("j")
    ? { ...form, pricing_mode: "JOURNALIER", daily_rate: p.unit_price }
    : { ...form, pricing_mode: "HORAIRE", hourly_rate: p.unit_price };
}

export function centerOptions(centers: TrainingCenter[], currentId: number | null, currentName: string | null) {
  const opts = centers.map((c) => ({ value: String(c.id), label: c.name }));
  if (currentId && !centers.some((c) => c.id === currentId)) opts.push({ value: String(currentId), label: `${currentName ?? ""} (hors liste)` });
  return [{ value: "", label: "-" }, ...opts];
}
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { DurationField, MoneyField, StatusBadge, Tabs } from "../../components/Fields";
import { AsyncSelect, Option } from "../../components/Pickers";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Card, Checkbox, ConfirmDialog, Loading, Modal, SelectField, TextAreaField, TextField } from "../../components/ui";
import { formatBytes, formatDate, formatMinutes, formatMoney, formatQty, parseQty, todayIso } from "../../lib/format";
import { searchContacts } from "../../lib/search";

const INPUT_KEYS: (keyof MissionInput)[] = [
  "title", "client_id", "contact_id", "center_id", "trainer_id", "software_id", "description", "start_date", "end_date", "os", "modality", "group_format",
  "provide_pc", "recording", "planned_minutes", "days_count", "schedule_text", "address", "room", "visio_link", "pricing_mode", "hourly_rate",
  "daily_rate", "flat_fee", "billable", "special_conditions", "internal_notes", "annotations", "file_created", "first_call_done", "invoice_sent", "objectives", "prerequisites",
  "program", "methods", "means", "evaluation_terms", "evaluation_result", "satisfaction", "trainer_comment",
];

// Fiche serveur -> formulaire (seuls les champs saisissables sont conservés).
function toInput(m: MissionDetail): MissionInput {
  const out = {} as Record<string, unknown>;
  for (const k of INPUT_KEYS) out[k] = m[k];
  return out as unknown as MissionInput;
}

// Onglets de la fiche.
type Tab = "infos" | "sessions" | "participants" | "frais" | "documents" | "pedagogie" | "facturation";

// Fiche mission : formulaire principal + onglets sessions / participants / frais / documents / pédagogie / facturation.
export function MissionPage() {
  const { id } = useParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const { data, isLoading } = useQuery({ queryKey: ["mission", id], queryFn: () => api.get<MissionDetail>(`/missions/${id}`) });
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people") });
  const { data: centers } = useQuery({ queryKey: ["training-centers"], queryFn: () => api.get<TrainingCenter[]>("/training-centers") });
  const { data: products } = useQuery({ queryKey: ["products"], queryFn: () => api.get<Product[]>("/products") });
  const { data: software } = useQuery({ queryKey: ["software"], queryFn: () => api.get<Software[]>("/software") });
  const [form, setForm] = useState<MissionInput | null>(null);
  const [client, setClient] = useState<Option | null>(null);
  const [contact, setContact] = useState<Option | null>(null);
  const [tab, setTab] = useState<Tab>("infos");
  const [error, setError] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    if (data) {
      setForm(toInput(data));
      setClient({ id: data.client_id, label: data.client_name });
      setContact(data.contact_id ? { id: data.contact_id, label: data.contact_name ?? "" } : null);
    }
  }, [data]);

  const onError = (e: unknown) => {
    setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    window.scrollTo({ top: 0, behavior: "smooth" });
  };
  const apply = (m: MissionDetail, msg?: string) => {
    qc.setQueryData(["mission", id], m);
    qc.invalidateQueries({ queryKey: ["missions"] });
    qc.invalidateQueries({ queryKey: ["missions-dashboard"] });
    setError("");
    if (msg) toast(msg);
  };
  // Enregistrement, changement de statut, suppression.
  const save = useMutation({ mutationFn: () => api.put<MissionDetail>(`/missions/${id}`, form), onSuccess: (m) => apply(m, "Mission enregistrée"), onError });
  const status = useMutation({ mutationFn: (s: MissionStatus) => api.post<MissionDetail>(`/missions/${id}/status`, { status: s }), onSuccess: (m) => apply(m, `Statut : ${MISSION_STATUS[m.status].label}`), onError });
  const remove = useMutation({ mutationFn: () => api.del(`/missions/${id}`), onSuccess: () => { qc.invalidateQueries({ queryKey: ["missions"] }); toast("Mission supprimée"); nav("/missions"); }, onError: (e) => { setConfirmDelete(false); onError(e); } });

  if (isLoading || !data || !form) return <Loading />;
  const dirty = JSON.stringify(toInput(data)) !== JSON.stringify(form);
  const set = <K extends keyof MissionInput>(k: K, v: MissionInput[K]) => setForm({ ...form, [k]: v });
  const txt = (k: keyof MissionInput) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => set(k, e.target.value as never);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate();
  };

  const tabs: [Tab, string][] = [
    ["infos", "Informations"],
    ["sessions", `Sessions (${data.sessions.length})`],
    ["participants", `Participants (${data.participants.length})`],
    ["frais", `Frais (${data.expenses.length})`],
    ["documents", `Documents (${data.documents.length})`],
    ["pedagogie", "Suivi pédagogique"],
    ["facturation", "Facturation"],
  ];

  return (
    <form onSubmit={submit}>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb"><Link to="/missions">Missions</Link><span>{data.reference}</span></div>
          {canEdit && dirty ? (
            <>
              <Button variant="primary" type="submit" loading={save.isPending}>Enregistrer</Button>
              <Button type="button" onClick={() => setForm(toInput(data))}>Annuler</Button>
            </>
          ) : null}
        </div>
        <div className="page-header-right">
          {canEdit && data.totals.uninvoiced_amount > 0 ? (
            <Button type="button" variant="primary" onClick={() => nav(`/facturation/depuis-projet/${data.project_id}`)}>
              Générer la facture ({formatMoney(data.totals.uninvoiced_amount)})
            </Button>
          ) : null}
          {canEdit ? <Button type="button" variant="ghost" className="text-danger" onClick={() => setConfirmDelete(true)}>Supprimer</Button> : null}
        </div>
      </div>

      <div className="statusbar">
        {MISSION_STATUS_ORDER.filter((s) => s !== "ANNULEE").map((s) => (
          <button key={s} type="button" disabled={!canEdit || status.isPending} className={`step ${data.status === s ? "active" : ""}`}
            onClick={() => data.status !== s && status.mutate(s)}>
            {MISSION_STATUS[s].label}
          </button>
        ))}
        <button type="button" disabled={!canEdit} className={`step danger ${data.status === "ANNULEE" ? "active" : ""}`} onClick={() => data.status !== "ANNULEE" && status.mutate("ANNULEE")}>
          Annulée
        </button>
      </div>

      <div className="page-body wide">
        <Alert>{error}</Alert>
        <div className="mission-head">
          <div>
            <h1 className="mission-title">{data.title}</h1>
            <div className="muted">
              <Link to={`/contacts/${data.client_id}`}>{data.client_name}</Link>
              {data.software_name ? ` · ${data.software_name}` : ""} · {MODALITY_LABELS[data.modality]} · {FORMAT_LABELS[data.group_format]}
              {data.trainer_name ? ` · Formateur : ${data.trainer_name}` : ""}
            </div>
          </div>
          <div className="stats compact">
            <div className="stat"><span className="stat-label">Prévu</span><span className="stat-value">{formatMoney(data.totals.planned_total)}</span><span className="stat-sub">{formatMinutes(data.planned_minutes)}</span></div>
            <div className="stat"><span className="stat-label">Réalisé</span><span className="stat-value">{formatMoney(data.totals.realized_amount)}</span><span className="stat-sub">{formatMinutes(data.totals.realized_minutes)}</span></div>
            <div className="stat"><span className="stat-label">Facturé HT</span><span className="stat-value">{formatMoney(data.totals.invoiced_ht)}</span></div>
            <div className={`stat ${data.totals.uninvoiced_amount ? "stat-warning" : ""}`}><span className="stat-label">À facturer</span><span className="stat-value">{formatMoney(data.totals.uninvoiced_amount)}</span></div>
          </div>
        </div>

        <Card>
          <Tabs tabs={tabs} value={tab} onChange={setTab} />
          {tab === "infos" ? (
            <fieldset disabled={!canEdit} className="plain form-stack">
              <h3 className="section-title">Identification</h3>
              <div className="grid-3">
                <TextField label="Intitulé de la formation" required value={form.title} onChange={txt("title")} wide />
                <AsyncSelect label="Client / entreprise" value={client} search={searchContacts} disabled={!canEdit} onChange={(o) => { setClient(o); set("client_id", o?.id ?? 0); }} />
                <AsyncSelect label="Contact principal" value={contact} search={(q) => searchContacts(q, { persons: true })} disabled={!canEdit} onChange={(o) => { setContact(o); set("contact_id", o?.id ?? null); }} />
                <SelectField label="Centre de formation" value={String(form.center_id ?? "")} onChange={(e) => set("center_id", e.target.value ? Number(e.target.value) : null)}
                  options={centerOptions(centers ?? [], data.center_id, data.center_name)} hint="Liste gérée dans Paramètres > Centres de formation" />
                <SelectField label="Formateur" value={String(form.trainer_id ?? "")} onChange={(e) => set("trainer_id", e.target.value ? Number(e.target.value) : null)}
                  options={[{ value: "", label: "-" }, ...(people ?? []).map((p) => ({ value: String(p.id), label: p.display_name }))]} />
                <SelectField label="Logiciel" value={String(form.software_id ?? "")} onChange={(e) => set("software_id", e.target.value ? Number(e.target.value) : null)}
                  options={[{ value: "", label: "-" }, ...(software ?? []).filter((s) => s.active || s.id === form.software_id).map((s) => ({ value: String(s.id), label: s.name }))]} />
                <SelectField label="OS" value={form.os} onChange={(e) => set("os", e.target.value as MissionInput["os"])} options={[{ value: "", label: "-" }, { value: "PC", label: "PC" }, { value: "MAC", label: "Mac" }]} />
                <SelectField label="Modalité" value={form.modality} onChange={(e) => set("modality", e.target.value as MissionInput["modality"])} options={Object.entries(MODALITY_LABELS).map(([value, label]) => ({ value, label }))} />
                <SelectField label="Format" value={form.group_format} onChange={(e) => set("group_format", e.target.value as MissionInput["group_format"])} options={Object.entries(FORMAT_LABELS).map(([value, label]) => ({ value, label }))} />
                <div className="field">
                  <label>Options</label>
                  <Checkbox label="Fournir un PC" checked={form.provide_pc} onChange={(e) => set("provide_pc", e.target.checked)} />
                  <Checkbox label="Enregistrer les sessions" checked={form.recording} onChange={(e) => set("recording", e.target.checked)} />
                </div>
                <div className="field">
                  <label>Suivi du dossier</label>
                  <Checkbox label="Dossier créé" checked={form.file_created} onChange={(e) => set("file_created", e.target.checked)} />
                  <Checkbox label="1er appel fait" checked={form.first_call_done} onChange={(e) => set("first_call_done", e.target.checked)} />
                  <Checkbox label="Facture envoyée" checked={form.invoice_sent} onChange={(e) => set("invoice_sent", e.target.checked)} />
                </div>
                <TextAreaField label="Description / objectifs" value={form.description} onChange={txt("description")} />
                <TextAreaField label="Notes internes" value={form.internal_notes} onChange={txt("internal_notes")} />
                <TextAreaField label="Annotations" value={form.annotations} onChange={txt("annotations")} rows={6} placeholder="Zone libre : remarques, historique des échanges, points à ne pas oublier..." />
              </div>
              <h3 className="section-title">Dates et lieux</h3>
              <div className="grid-3">
                <TextField label="Date de début" type="date" value={form.start_date ?? ""} onChange={(e) => set("start_date", e.target.value || null)} />
                <TextField label="Date de fin" type="date" value={form.end_date ?? ""} onChange={(e) => set("end_date", e.target.value || null)} />
                <TextField label="Nombre de jours" type="number" min={0} value={String(form.days_count)} onChange={(e) => set("days_count", Number(e.target.value) || 0)} />
                <DurationField label="Heures prévues" value={form.planned_minutes} onChange={(v) => set("planned_minutes", v)} disabled={!canEdit} />
                <TextField label="Horaires" value={form.schedule_text} onChange={txt("schedule_text")} placeholder="ex. 9h00-12h30 / 13h30-17h00" />
                <TextField label="Salle" value={form.room} onChange={txt("room")} />
                <TextField label="Adresse du lieu de formation" value={form.address} onChange={txt("address")} wide />
                <TextField label="Lien Teams / visioconférence" value={form.visio_link} onChange={txt("visio_link")} wide />
              </div>
              <h3 className="section-title">Données commerciales</h3>
              <div className="grid-3">
                <SelectField label="Tarification" value={form.pricing_mode} onChange={(e) => set("pricing_mode", e.target.value as MissionInput["pricing_mode"])} options={Object.entries(PRICING_LABELS).map(([value, label]) => ({ value, label }))} />
                <SelectField label="Produit (remplit le tarif)" value="" onChange={(e) => setForm(applyProductRate(form, products ?? [], e.target.value))} options={productOptions(products ?? [])} hint="Liste : Paramètres > Produits" />
                <MoneyField label="Tarif horaire" value={form.hourly_rate} onChange={(v) => set("hourly_rate", v)} disabled={!canEdit} />
                <MoneyField label="Tarif journalier" value={form.daily_rate} onChange={(v) => set("daily_rate", v)} disabled={!canEdit} hint="1 jour = 7 h" />
                <MoneyField label="Forfait mission" value={form.flat_fee} onChange={(v) => set("flat_fee", v)} disabled={!canEdit} />
                <div className="field"><label>Facturation</label><Checkbox label="Mission facturable" checked={form.billable} onChange={(e) => set("billable", e.target.checked)} /></div>
                <div className="field"><label>Montant total prévu</label><div className="readonly">{formatMoney(data.totals.planned_total)} <span className="muted small">(dont frais {formatMoney(data.totals.expenses_total)})</span></div></div>
                <TextAreaField label="Conditions particulières" value={form.special_conditions} onChange={txt("special_conditions")} />
              </div>
            </fieldset>
          ) : null}
          {tab === "sessions" ? <SessionsTab m={data} canEdit={canEdit} people={people ?? []} onChange={apply} onError={onError} /> : null}
          {tab === "participants" ? <ParticipantsTab m={data} canEdit={canEdit} onChange={apply} onError={onError} /> : null}
          {tab === "frais" ? <ExpensesTab m={data} canEdit={canEdit} onChange={apply} onError={onError} /> : null}
          {tab === "documents" ? <DocumentsTab m={data} canEdit={canEdit} onChange={apply} onError={onError} /> : null}
          {tab === "pedagogie" ? (
            <fieldset disabled={!canEdit} className="plain grid-2">
              <TextAreaField label="Objectifs pédagogiques" value={form.objectives} onChange={txt("objectives")} />
              <TextAreaField label="Prérequis" value={form.prerequisites} onChange={txt("prerequisites")} />
              <TextAreaField label="Programme" rows={6} value={form.program} onChange={txt("program")} />
              <TextAreaField label="Méthodes pédagogiques" value={form.methods} onChange={txt("methods")} />
              <TextAreaField label="Moyens pédagogiques" value={form.means} onChange={txt("means")} />
              <TextAreaField label="Modalités d'évaluation" value={form.evaluation_terms} onChange={txt("evaluation_terms")} />
              <TextAreaField label="Résultat d'évaluation" value={form.evaluation_result} onChange={txt("evaluation_result")} />
              <TextAreaField label="Satisfaction stagiaire" value={form.satisfaction} onChange={txt("satisfaction")} />
              <TextAreaField label="Commentaire du formateur" value={form.trainer_comment} onChange={txt("trainer_comment")} />
            </fieldset>
          ) : null}
          {tab === "facturation" ? (
            <div className="form-stack">
              <dl className="details wide-details">
                <dt>Prestation prévue</dt><dd>{formatMoney(data.totals.planned_fee)} ({PRICING_LABELS[data.pricing_mode]})</dd>
                <dt>Frais</dt><dd>{formatMoney(data.totals.expenses_total)}</dd>
                <dt>Réalisé (temps)</dt><dd>{formatMoney(data.totals.realized_amount)} pour {formatMinutes(data.totals.realized_minutes)}</dd>
                <dt>Déjà facturé HT</dt><dd>{formatMoney(data.totals.invoiced_ht)}</dd>
                <dt>Reste à facturer</dt><dd><strong>{formatMoney(data.totals.uninvoiced_amount)}</strong></dd>
              </dl>
              <p className="muted small">Seules les sessions marquées « réalisées » et les frais facturables sont proposés à la facturation. Un élément facturé ne peut plus l'être une seconde fois.</p>
              {canEdit && data.totals.uninvoiced_amount > 0 ? <div><Button type="button" variant="primary" onClick={() => nav(`/facturation/depuis-projet/${data.project_id}`)}>Générer la facture</Button></div> : null}
              <DataTable
                columns={[
                  { key: "n", header: "Numéro", render: (i) => <Link to={`/facturation/${i.id}`}>{i.number ?? "Brouillon"}</Link> },
                  { key: "k", header: "Type", render: (i) => (i.kind === "AVOIR" ? "Avoir" : "Facture") },
                  { key: "d", header: "Date", render: (i) => formatDate(i.invoice_date) },
                  { key: "t", header: "Total", align: "right", render: (i) => formatMoney(i.total_ttc) },
                  { key: "s", header: "Statut", render: (i) => <StatusBadge map={INVOICE_STATUS} value={i.status} /> },
                ]}
                rows={data.invoices}
                rowKey={(i) => i.id}
                empty="Aucune facture"
              />
            </div>
          ) : null}
        </Card>
      </div>
      <ConfirmDialog open={confirmDelete} title="Supprimer la mission" message="Possible uniquement si la mission n'a ni temps ni facture. Sinon, passez-la au statut « Annulée »."
        confirmLabel="Supprimer" danger loading={remove.isPending} onConfirm={() => remove.mutate()} onCancel={() => setConfirmDelete(false)} />
    </form>
  );
}

// Props communes aux onglets : mission, droits, rappels de mise à jour / erreur.
interface TabProps {
  m: MissionDetail;
  canEdit: boolean;
  onChange: (m: MissionDetail, msg?: string) => void;
  onError: (e: unknown) => void;
}

// Rendu des statuts de séance.
const SESSION_STATUS = {
  PREVUE: { label: "Prévue", tone: "info" as const },
  REALISEE: { label: "Réalisée", tone: "success" as const },
  ANNULEE: { label: "Annulée", tone: "danger" as const },
};

// Séances : ajout / modification, marquage "Réalisée" avec présences, annulation.
function SessionsTab({ m, canEdit, people, onChange, onError }: TabProps & { people: Person[] }) {
  type SForm = { id?: number; date: string; start_time: string; end_time: string; break_minutes: number; trainer_id: number | null; location: string; notes: string };
  const [edit, setEdit] = useState<SForm | null>(null);
  const [realize, setRealize] = useState<Session | null>(null);
  const [absents, setAbsents] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);
  const run = async (p: Promise<MissionDetail>, msg: string) => {
    setBusy(true);
    try {
      onChange(await p, msg);
      setEdit(null);
      setRealize(null);
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };
  const lastDate = m.sessions.length ? m.sessions[m.sessions.length - 1].date : m.start_date ?? todayIso();
  const columns: Column<Session>[] = [
    { key: "date", header: "Date", render: (s) => new Date(`${s.date}T12:00:00`).toLocaleDateString("fr-FR", { weekday: "short", day: "2-digit", month: "2-digit", year: "numeric" }) },
    { key: "h", header: "Horaires", render: (s) => `${s.start_time} - ${s.end_time}${s.break_minutes ? ` (pause ${s.break_minutes} min)` : ""}` },
    { key: "d", header: "Durée", align: "right", render: (s) => formatMinutes(s.duration_minutes) },
    { key: "t", header: "Formateur", render: (s) => s.trainer_name ?? "" },
    { key: "l", header: "Lieu", render: (s) => s.location },
    { key: "s", header: "Statut", render: (s) => <>{<StatusBadge map={SESSION_STATUS} value={s.status} />} {s.invoiced ? <Badge tone="success">Facturée</Badge> : null}</> },
    {
      key: "a",
      header: "",
      align: "right",
      render: (s) => canEdit && !s.invoiced ? (
        <span className="row-actions">
          {s.status !== "ANNULEE" ? <Button type="button" variant="ghost" onClick={() => { setRealize(s); setAbsents(m.attendance.filter((a) => a.session_id === s.id && !a.present).map((a) => a.participant_id)); }}>{s.status === "REALISEE" ? "Présences" : "Réalisée"}</Button> : null}
          <Button type="button" variant="ghost" onClick={() => setEdit({ id: s.id, date: s.date, start_time: s.start_time, end_time: s.end_time, break_minutes: s.break_minutes, trainer_id: s.trainer_id, location: s.location, notes: s.notes })}>Modifier</Button>
          {s.status !== "PREVUE" ? <Button type="button" variant="ghost" onClick={() => run(api.post(`/sessions/${s.id}/status`, { status: "PREVUE" }), "Session remise à prévue")}>Remettre prévue</Button>
            : <Button type="button" variant="ghost" onClick={() => run(api.post(`/sessions/${s.id}/status`, { status: "ANNULEE" }), "Session annulée")}>Annuler</Button>}
          <Button type="button" variant="ghost" className="text-danger" onClick={() => run(api.del(`/sessions/${s.id}`), "Session supprimée")}>Supprimer</Button>
        </span>
      ) : null,
    },
  ];
  const total = m.sessions.filter((s) => s.status !== "ANNULEE").reduce((a, s) => a + s.duration_minutes, 0);
  return (
    <div className="form-stack">
      <div className="toolbar between">
        <span className="muted">Total des sessions : <strong>{formatMinutes(total)}</strong> {m.planned_minutes ? `/ ${formatMinutes(m.planned_minutes)} prévues` : ""}</span>
        {canEdit ? <Button type="button" variant="primary" onClick={() => setEdit({ date: m.sessions.length ? nextWorkday(lastDate) : lastDate, start_time: "09:00", end_time: "17:00", break_minutes: 60, trainer_id: m.trainer_id, location: m.address, notes: "" })}>Ajouter une session</Button> : null}
      </div>
      <p className="muted small">Marquer une session « réalisée » crée automatiquement la ligne de temps du formateur (une seule, même si vous recommencez) et enregistre les présences.</p>
      <DataTable columns={columns} rows={m.sessions} rowKey={(s) => s.id} empty="Aucune session planifiée" />
      <Modal open={!!edit} title={edit?.id ? "Modifier la session" : "Nouvelle session"} onClose={() => setEdit(null)}
        footer={<><Button type="button" onClick={() => setEdit(null)}>Annuler</Button><Button type="button" variant="primary" loading={busy}
          onClick={() => edit && run(edit.id ? api.put<MissionDetail>(`/sessions/${edit.id}`, edit) : api.post<MissionDetail>(`/missions/${m.id}/sessions`, edit), "Session enregistrée")}>Enregistrer</Button></>}>
        {edit ? (
          <div className="grid-2">
            <TextField label="Date" type="date" required value={edit.date} onChange={(e) => setEdit({ ...edit, date: e.target.value })} />
            <SelectField label="Formateur" value={String(edit.trainer_id ?? "")} onChange={(e) => setEdit({ ...edit, trainer_id: e.target.value ? Number(e.target.value) : null })}
              options={[{ value: "", label: "Formateur de la mission" }, ...people.map((p) => ({ value: String(p.id), label: p.display_name }))]} />
            <TextField label="Début" type="time" value={edit.start_time} onChange={(e) => setEdit({ ...edit, start_time: e.target.value })} />
            <TextField label="Fin" type="time" value={edit.end_time} onChange={(e) => setEdit({ ...edit, end_time: e.target.value })} />
            <TextField label="Pause (minutes)" type="number" min={0} value={String(edit.break_minutes)} onChange={(e) => setEdit({ ...edit, break_minutes: Number(e.target.value) || 0 })} />
            <TextField label="Lieu" value={edit.location} onChange={(e) => setEdit({ ...edit, location: e.target.value })} />
            <TextAreaField label="Notes" value={edit.notes} onChange={(e) => setEdit({ ...edit, notes: e.target.value })} />
          </div>
        ) : null}
      </Modal>
      <Modal open={!!realize} title="Session réalisée : présences" onClose={() => setRealize(null)}
        footer={<><Button type="button" onClick={() => setRealize(null)}>Annuler</Button><Button type="button" variant="primary" loading={busy}
          onClick={() => realize && run(api.post<MissionDetail>(`/sessions/${realize.id}/realize`, { absent_participant_ids: absents }), "Session réalisée, temps enregistré")}>Valider</Button></>}>
        {realize ? (
          <div className="form-stack">
            <p>{formatDate(realize.date)}, {realize.start_time} - {realize.end_time} ({formatMinutes(realize.duration_minutes)})</p>
            {m.participants.length === 0 ? <p className="muted">Aucun participant inscrit.</p> : <p className="muted small">Décochez les absents.</p>}
            {m.participants.map((p) => (
              <Checkbox key={p.id} label={`${p.name} ${p.first_name}`.trim()} checked={!absents.includes(p.id)}
                onChange={(e) => setAbsents(e.target.checked ? absents.filter((x) => x !== p.id) : [...absents, p.id])} />
            ))}
          </div>
        ) : null}
      </Modal>
    </div>
  );
}

// Jour ouvré suivant (pré-remplissage de la séance suivante).
function nextWorkday(iso: string) {
  const [y, mo, d] = iso.split("-").map(Number);
  const dt = new Date(y, mo - 1, d + 1);
  while (dt.getDay() === 0 || dt.getDay() === 6) dt.setDate(dt.getDate() + 1);
  return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
}

// Rendu de la progression d'un stagiaire.
const PROGRESS = { EN_COURS: { label: "En cours", tone: "info" as const }, TERMINE: { label: "Terminé", tone: "success" as const }, ABANDON: { label: "Abandon", tone: "danger" as const } };

// Stagiaires : inscription depuis les contacts, suivi pédagogique, présences cumulées.
function ParticipantsTab({ m, canEdit, onChange, onError }: TabProps) {
  const [pick, setPick] = useState<Option | null>(null);
  const [edit, setEdit] = useState<Participant | null>(null);
  const run = async (p: Promise<MissionDetail>, msg: string) => {
    try {
      onChange(await p, msg);
      setEdit(null);
    } catch (e) {
      onError(e);
    }
  };
  const columns: Column<Participant>[] = [
    { key: "n", header: "Nom", render: (p) => <Link to={`/contacts/${p.partner_id}`}>{`${p.name} ${p.first_name}`.trim()}</Link>, sortValue: (p) => p.name },
    { key: "c", header: "Entreprise", render: (p) => p.company ?? "" },
    { key: "e", header: "Email", render: (p) => p.email },
    { key: "t", header: "Téléphone", render: (p) => <span className="nowrap">{p.phone}</span> },
    { key: "f", header: "Fonction", render: (p) => p.job_title },
    { key: "h", header: "Heures réalisées", align: "right", render: (p) => formatMinutes(p.minutes_done) },
    { key: "pa", header: "Présences", align: "center", render: (p) => `${p.sessions_present} / ${p.sessions_present + p.sessions_absent}` },
    { key: "s", header: "Statut", render: (p) => <StatusBadge map={PROGRESS} value={p.progress_status} /> },
    { key: "ev", header: "Évaluation", render: (p) => <span className="small">{p.evaluation}</span> },
    { key: "a", header: "", align: "right", render: (p) => canEdit ? <span className="row-actions"><Button type="button" variant="ghost" onClick={() => setEdit(p)}>Modifier</Button><Button type="button" variant="ghost" className="text-danger" onClick={() => run(api.del(`/participants/${p.id}`), "Participant retiré")}>Retirer</Button></span> : null },
  ];
  return (
    <div className="form-stack">
      {canEdit ? (
        <div className="inline-form wide-inline">
          <AsyncSelect label="Ajouter un stagiaire (contact existant)" value={pick} search={(q) => searchContacts(q, { persons: true })} onChange={setPick} />
          <Button type="button" variant="primary" disabled={!pick} onClick={() => pick && run(api.post<MissionDetail>(`/missions/${m.id}/participants`, { partner_id: pick.id }), "Stagiaire ajouté").then(() => setPick(null))}>Ajouter</Button>
          <Link className="btn btn-ghost" to={`/contacts/nouveau?parent_id=${m.client_id}&parent_name=${encodeURIComponent(m.client_name)}`}>Créer un contact</Link>
        </div>
      ) : null}
      <DataTable columns={columns} rows={m.participants} rowKey={(p) => p.id} empty="Aucun participant" />
      <Modal open={!!edit} title="Participant" onClose={() => setEdit(null)}
        footer={<><Button type="button" onClick={() => setEdit(null)}>Annuler</Button><Button type="button" variant="primary"
          onClick={() => edit && run(api.put<MissionDetail>(`/participants/${edit.id}`, { progress_status: edit.progress_status, evaluation: edit.evaluation, comment: edit.comment }), "Participant enregistré")}>Enregistrer</Button></>}>
        {edit ? (
          <div className="form-stack">
            <strong>{`${edit.name} ${edit.first_name}`.trim()}</strong>
            <SelectField label="Statut" value={edit.progress_status} onChange={(e) => setEdit({ ...edit, progress_status: e.target.value as Participant["progress_status"] })}
              options={Object.entries(PROGRESS).map(([value, s]) => ({ value, label: s.label }))} />
            <TextAreaField label="Évaluation" value={edit.evaluation} onChange={(e) => setEdit({ ...edit, evaluation: e.target.value })} />
            <TextAreaField label="Commentaires" value={edit.comment} onChange={(e) => setEdit({ ...edit, comment: e.target.value })} />
          </div>
        ) : null}
      </Modal>
    </div>
  );
}

// Frais : nature, quantité x prix, refacturable ; verrouillés une fois facturés.
function ExpensesTab({ m, canEdit, onChange, onError }: TabProps) {
  type EForm = { id?: number; kind: Expense["kind"]; label: string; expense_date: string; qty: string; unit_price: number; billable: boolean };
  const empty: EForm = { kind: "DEPLACEMENT", label: "", expense_date: "", qty: "1", unit_price: 0, billable: true };
  const [edit, setEdit] = useState<EForm | null>(null);
  const [err, setErr] = useState("");
  const submit = async () => {
    if (!edit) return;
    const q = parseQty(edit.qty);
    if (q === null || q <= 0) {
      setErr("Quantité invalide");
      return;
    }
    const body = { kind: edit.kind, label: edit.label, expense_date: edit.expense_date || null, quantity_milli: q, unit_price: edit.unit_price, billable: edit.billable };
    try {
      onChange(await (edit.id ? api.put<MissionDetail>(`/expenses/${edit.id}`, body) : api.post<MissionDetail>(`/missions/${m.id}/expenses`, body)), "Frais enregistré");
      setEdit(null);
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : "Erreur inattendue");
    }
  };
  const columns: Column<Expense>[] = [
    { key: "k", header: "Type", render: (e) => EXPENSE_LABELS[e.kind] },
    { key: "l", header: "Libellé", render: (e) => e.label },
    { key: "d", header: "Date", render: (e) => formatDate(e.expense_date) },
    { key: "q", header: "Quantité", align: "right", render: (e) => `${formatQty(e.quantity_milli)}${e.kind === "KILOMETRIQUE" ? " km" : ""}` },
    { key: "u", header: "Prix unitaire", align: "right", render: (e) => formatMoney(e.unit_price) },
    { key: "a", header: "Montant", align: "right", render: (e) => formatMoney(e.amount) },
    { key: "b", header: "", render: (e) => <>{!e.billable ? <Badge>Non facturable</Badge> : null} {e.invoiced ? <Badge tone="success">Facturé</Badge> : null}</> },
    {
      key: "x", header: "", align: "right",
      render: (e) => canEdit && !e.invoiced ? (
        <span className="row-actions">
          <Button type="button" variant="ghost" onClick={() => { setErr(""); setEdit({ id: e.id, kind: e.kind, label: e.label, expense_date: e.expense_date ?? "", qty: formatQty(e.quantity_milli), unit_price: e.unit_price, billable: e.billable }); }}>Modifier</Button>
          <Button type="button" variant="ghost" className="text-danger" onClick={async () => { try { onChange(await api.del<MissionDetail>(`/expenses/${e.id}`), "Frais supprimé"); } catch (x) { onError(x); } }}>Supprimer</Button>
        </span>
      ) : null,
    },
  ];
  return (
    <div className="form-stack">
      <div className="toolbar between">
        <span className="muted">Total des frais : <strong>{formatMoney(m.totals.expenses_total)}</strong></span>
        {canEdit ? <Button type="button" variant="primary" onClick={() => { setErr(""); setEdit(empty); }}>Ajouter un frais</Button> : null}
      </div>
      <DataTable columns={columns} rows={m.expenses} rowKey={(e) => e.id} empty="Aucun frais" />
      <Modal open={!!edit} title={edit?.id ? "Modifier le frais" : "Nouveau frais"} onClose={() => setEdit(null)}
        footer={<><Button type="button" onClick={() => setEdit(null)}>Annuler</Button><Button type="button" variant="primary" onClick={submit}>Enregistrer</Button></>}>
        {edit ? (
          <div className="form-stack">
            <Alert>{err}</Alert>
            <div className="grid-2">
              <SelectField label="Type" value={edit.kind} onChange={(e) => setEdit({ ...edit, kind: e.target.value as Expense["kind"] })} options={Object.entries(EXPENSE_LABELS).map(([value, label]) => ({ value, label }))} />
              <TextField label="Date" type="date" value={edit.expense_date} onChange={(e) => setEdit({ ...edit, expense_date: e.target.value })} />
              <TextField label="Libellé" value={edit.label} onChange={(e) => setEdit({ ...edit, label: e.target.value })} wide />
              <TextField label={edit.kind === "KILOMETRIQUE" ? "Kilomètres" : "Quantité"} value={edit.qty} onChange={(e) => setEdit({ ...edit, qty: e.target.value })} inputMode="decimal" />
              <MoneyField label={edit.kind === "KILOMETRIQUE" ? "Prix par km" : "Prix unitaire"} value={edit.unit_price} onChange={(v) => setEdit({ ...edit, unit_price: v })} />
              <Checkbox label="Refacturable au client" checked={edit.billable} onChange={(e) => setEdit({ ...edit, billable: e.target.checked })} />
            </div>
          </div>
        ) : null}
      </Modal>
    </div>
  );
}

// Pièces jointes : envoi par catégorie, téléchargement, suppression.
function DocumentsTab({ m, canEdit, onChange, onError }: TabProps) {
  const [category, setCategory] = useState<keyof typeof DOC_LABELS>("CONVENTION");
  const [busy, setBusy] = useState(false);
  const qc = useQueryClient();
  const upload = async (file: File) => {
    const fd = new FormData();
    fd.append("category", category);
    fd.append("file", file);
    setBusy(true);
    try {
      onChange(await api.post<MissionDetail>(`/missions/${m.id}/documents`, fd), "Document ajouté");
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };
  const remove = async (id: number) => {
    try {
      await api.del(`/documents/${id}`);
      qc.invalidateQueries({ queryKey: ["mission", String(m.id)] });
    } catch (e) {
      onError(e);
    }
  };
  return (
    <div className="form-stack">
      {canEdit ? (
        <div className="toolbar">
          <SelectField label="Catégorie" value={category} onChange={(e) => setCategory(e.target.value as keyof typeof DOC_LABELS)} options={Object.entries(DOC_LABELS).map(([value, label]) => ({ value, label }))} />
          <label className="btn btn-primary">
            {busy ? "Envoi..." : "Ajouter un fichier (20 Mo max)"}
            <input type="file" hidden onChange={(e) => { const f = e.target.files?.[0]; if (f) upload(f); e.target.value = ""; }} />
          </label>
        </div>
      ) : null}
      <DataTable
        columns={[
          { key: "c", header: "Catégorie", render: (d) => DOC_LABELS[d.category] ?? d.category },
          { key: "n", header: "Fichier", render: (d) => <a href={`/api/documents/${d.id}`} download>{d.file_name}</a> },
          { key: "s", header: "Taille", align: "right", render: (d) => formatBytes(d.size) },
          { key: "u", header: "Ajouté par", render: (d) => d.uploaded_by_name ?? "" },
          { key: "d", header: "Le", render: (d) => formatDate(d.created_at) },
          { key: "x", header: "", align: "right", render: (d) => (canEdit ? <Button type="button" variant="ghost" className="text-danger" onClick={() => remove(d.id)}>Supprimer</Button> : null) },
        ]}
        rows={m.documents}
        rowKey={(d) => d.id}
        empty="Aucun document"
      />
    </div>
  );
}
