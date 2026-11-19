import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import {
  FORMAT_LABELS,
  MISSION_STATUS,
  MISSION_STATUS_ORDER,
  MODALITY_LABELS,
  PRICING_LABELS,
  type CalendarSession,
  type MissionDetail,
  type MissionListItem,
  type MissionStatus,
  type Page,
  type Person,
  type Product,
  type Software,
  type TrainingCenter,
} from "../../api/types";
import { applyProductRate, centerOptions, productOptions } from "./MissionPage";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { DurationField, MoneyField, Stat, StatusBadge } from "../../components/Fields";
import { AsyncSelect, Option } from "../../components/Pickers";
import { useToast } from "../../components/Toast";
import { Alert, Button, Loading, Modal, PageHeader, SearchInput, SelectField, TextField } from "../../components/ui";
import { addDaysIso, formatDate, formatMinutes, formatMoney, todayIso } from "../../lib/format";
import { useDebounced, useUrlState } from "../../lib/hooks";
import { searchContacts } from "../../lib/search";

// Indicateurs renvoyés par /missions/dashboard.
interface Dashboard {
  buckets: Record<string, number>;
  planned_revenue: number;
  realized_revenue: number;
  planned_minutes: number;
  realized_minutes: number;
  trainees: number;
  to_invoice_count: number;
  to_invoice_amount: number;
}

const BUCKETS: [string, string][] = [
  ["a_planifier", "À planifier"],
  ["confirmees", "Confirmées"],
  ["semaine", "Cette semaine"],
  ["en_cours", "En cours"],
  ["realisees", "Réalisées"],
  ["a_facturer", "À facturer"],
  ["facturees", "Facturées"],
];

// Les trois vues de l'écran.
type View = "liste" | "kanban" | "calendrier";

// Écran Missions : filtres (URL), indicateurs cliquables, vue liste / kanban / calendrier, création.
export function MissionsListPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const [f, setF] = useUrlState({ q: "", bucket: "", status: "", trainer_id: "", software_id: "", client_id: "", center_id: "", from: "", to: "", vue: "liste" });
  const [text, setText] = useState(f.q);
  const q = useDebounced(text, 250);
  useEffect(() => {
    if (q !== f.q) setF({ q });
  }, [q]); // eslint-disable-line react-hooks/exhaustive-deps
  const [client, setClient] = useState<Option | null>(null);
  const { data: centers } = useQuery({ queryKey: ["training-centers"], queryFn: () => api.get<TrainingCenter[]>("/training-centers") });

  const params = new URLSearchParams({ per_page: "500" });
  for (const [k, v] of Object.entries(f)) if (v && k !== "vue") params.set(k, v);
  const dashParams = new URLSearchParams(params);
  dashParams.delete("bucket");
  const { data, isLoading } = useQuery({ queryKey: ["missions", params.toString()], queryFn: () => api.get<Page<MissionListItem>>(`/missions?${params}`), placeholderData: (p) => p });
  const { data: dash } = useQuery({ queryKey: ["missions-dashboard", dashParams.toString()], queryFn: () => api.get<Dashboard>(`/missions/dashboard?${dashParams}`) });
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people") });
  const { data: software } = useQuery({ queryKey: ["software"], queryFn: () => api.get<Software[]>("/software") });

  // Déplacement d'une carte du kanban = changement de statut côté serveur.
  const setStatus = useMutation({
    mutationFn: ({ id, status }: { id: number; status: MissionStatus }) => api.post<MissionDetail>(`/missions/${id}/status`, { status }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["missions"] });
      qc.invalidateQueries({ queryKey: ["missions-dashboard"] });
    },
    onError: (e) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"),
  });

  const view = f.vue as View;
  const [creating, setCreating] = useState(false);

  return (
    <>
      <PageHeader title="Missions de formation" actions={canEdit ? <Button variant="primary" onClick={() => setCreating(true)}>Nouvelle mission</Button> : undefined}>
        <SearchInput value={text} onChange={setText} placeholder="Intitulé, référence, client, stagiaire..." />
        <div className="segmented small-seg">
          {(["liste", "kanban", "calendrier"] as View[]).map((v) => (
            <label key={v} className={view === v ? "active" : ""}>
              <input type="radio" name="vue" checked={view === v} onChange={() => setF({ vue: v })} />
              {v === "liste" ? "Liste" : v === "kanban" ? "Kanban" : "Calendrier"}
            </label>
          ))}
        </div>
      </PageHeader>
      <div className="filter-bar">
        <div className="filter-picker">
          <AsyncSelect label="Client" value={client} search={searchContacts} placeholder="Tous les clients" onChange={(o) => { setClient(o); setF({ client_id: o ? String(o.id) : "" }); }} />
        </div>
        <select aria-label="Centre de formation" value={f.center_id} onChange={(e) => setF({ center_id: e.target.value })}>
          <option value="">Tous les centres</option>
          {(centers ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
        </select>
        <select aria-label="Formateur" value={f.trainer_id} onChange={(e) => setF({ trainer_id: e.target.value })}>
          <option value="">Tous les formateurs</option>
          {(people ?? []).map((p) => <option key={p.id} value={p.id}>{p.display_name}</option>)}
        </select>
        <select aria-label="Formation" value={f.software_id} onChange={(e) => setF({ software_id: e.target.value })}>
          <option value="">Tous les logiciels</option>
          {(software ?? []).map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <select aria-label="Statut" value={f.status} onChange={(e) => setF({ status: e.target.value })}>
          <option value="">Tous les statuts</option>
          {MISSION_STATUS_ORDER.map((s) => <option key={s} value={s}>{MISSION_STATUS[s].label}</option>)}
        </select>
        <label className="inline-label">Du <input type="date" value={f.from} onChange={(e) => setF({ from: e.target.value })} /></label>
        <label className="inline-label">au <input type="date" value={f.to} onChange={(e) => setF({ to: e.target.value })} /></label>
      </div>
      <div className="page-body wide">
        {dash ? (
          <>
            <div className="stats">
              {BUCKETS.map(([k, label]) => (
                <Stat key={k} label={label} value={dash.buckets[k] ?? 0} tone={f.bucket === k ? "success" : k === "a_facturer" && dash.buckets[k] ? "warning" : undefined}
                  onClick={() => setF({ bucket: f.bucket === k ? "" : k })} />
              ))}
            </div>
            <div className="stats">
              <Stat label="CA prévu" value={formatMoney(dash.planned_revenue)} />
              <Stat label="CA réalisé" value={formatMoney(dash.realized_revenue)} />
              <Stat label="Heures prévues" value={formatMinutes(dash.planned_minutes)} />
              <Stat label="Heures réalisées" value={formatMinutes(dash.realized_minutes)} />
              <Stat label="Stagiaires" value={dash.trainees} />
              <Stat label="Restant à facturer" value={formatMoney(dash.to_invoice_amount)} sub={`${dash.to_invoice_count} mission(s)`} tone={dash.to_invoice_amount ? "warning" : undefined} />
            </div>
          </>
        ) : null}
        {isLoading || !data ? <Loading /> : view === "kanban" ? (
          <Kanban items={data.items} canEdit={canEdit} onMove={(id, status) => setStatus.mutate({ id, status })} onOpen={(id) => nav(`/missions/${id}`)} />
        ) : view === "calendrier" ? (
          <Calendar onOpen={(id) => nav(`/missions/${id}`)} />
        ) : (
          <MissionsTable items={data.items} onOpen={(id) => nav(`/missions/${id}`)} />
        )}
      </div>
      {creating ? <NewMissionModal onClose={() => setCreating(false)} onCreated={(m) => { toast("Mission créée"); nav(`/missions/${m.id}`); }} /> : null}
    </>
  );
}

// Texte "du .. au .." d'une mission (sessions, sinon dates du projet).
function period(m: MissionListItem) {
  const a = m.first_session ?? m.start_date;
  const b = m.last_session ?? m.end_date;
  if (!a) return "";
  return a === b || !b ? formatDate(a) : `${formatDate(a)} - ${formatDate(b)}`;
}

// Vue liste.
function MissionsTable({ items, onOpen }: { items: MissionListItem[]; onOpen: (id: number) => void }) {
  const columns: Column<MissionListItem>[] = [
    { key: "ref", header: "Référence", width: "120px", render: (m) => m.reference, sortValue: (m) => m.reference },
    { key: "title", header: "Formation", render: (m) => <><strong>{m.title}</strong><div className="muted small">{[m.software_name, MODALITY_LABELS[m.modality], FORMAT_LABELS[m.group_format]].filter(Boolean).join(" · ")}</div></>, sortValue: (m) => m.title },
    { key: "client", header: "Client", render: (m) => m.client_name, sortValue: (m) => m.client_name },
    { key: "center", header: "Centre", render: (m) => m.center_name ?? "", sortValue: (m) => m.center_name ?? "" },
    { key: "trainer", header: "Formateur", render: (m) => m.trainer_name ?? "" },
    { key: "period", header: "Dates", render: period, sortValue: (m) => m.first_session ?? m.start_date ?? "" },
    { key: "hours", header: "Heures", align: "right", render: (m) => `${formatMinutes(m.realized_minutes)} / ${formatMinutes(m.planned_minutes)}` },
    { key: "trainees", header: "Stagiaires", align: "right", render: (m) => m.participants },
    { key: "todo", header: "À facturer", align: "right", render: (m) => (m.uninvoiced_amount ? formatMoney(m.uninvoiced_amount) : "") , sortValue: (m) => m.uninvoiced_amount },
    { key: "status", header: "Statut", render: (m) => <StatusBadge map={MISSION_STATUS} value={m.status} />, sortValue: (m) => MISSION_STATUS_ORDER.indexOf(m.status) },
  ];
  return <DataTable columns={columns} rows={items} rowKey={(m) => m.id} onRowClick={(m) => onOpen(m.id)} empty="Aucune mission" />;
}

// Vue kanban : une colonne par statut, glisser-déposer pour changer de statut.
function Kanban({ items, canEdit, onMove, onOpen }: { items: MissionListItem[]; canEdit: boolean; onMove: (id: number, s: MissionStatus) => void; onOpen: (id: number) => void }) {
  const [drag, setDrag] = useState<number | null>(null);
  const cols = MISSION_STATUS_ORDER.filter((s) => s !== "BROUILLON" || items.some((m) => m.status === "BROUILLON"));
  return (
    <div className="kanban">
      {cols.map((s) => {
        const list = items.filter((m) => m.status === s);
        return (
          <div key={s} className="kanban-col" onDragOver={(e) => canEdit && e.preventDefault()}
            onDrop={() => { if (drag !== null) { const m = items.find((x) => x.id === drag); if (m && m.status !== s) onMove(drag, s); } setDrag(null); }}>
            <div className="kanban-head"><StatusBadge map={MISSION_STATUS} value={s} /> <span className="muted">{list.length}</span></div>
            {list.map((m) => (
              <div key={m.id} className="kanban-card" draggable={canEdit} onDragStart={() => setDrag(m.id)} onClick={() => onOpen(m.id)}
                role="button" tabIndex={0} onKeyDown={(e) => e.key === "Enter" && onOpen(m.id)}>
                <strong>{m.title}</strong>
                <div className="small">{m.client_name}</div>
                <div className="muted small">{m.reference} · {period(m) || "à planifier"}</div>
                <div className="muted small">{m.participants} stagiaire(s) · {formatMinutes(m.planned_minutes)}</div>
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}

// Premier jour du mois d'une date ISO.
function monthStart(iso: string) {
  return `${iso.slice(0, 7)}-01`;
}

// Vue calendrier mensuel des séances (/missions/calendar).
function Calendar({ onOpen }: { onOpen: (missionId: number) => void }) {
  const [month, setMonth] = useState(monthStart(todayIso()));
  const days = useMemo(() => {
    const [y, m] = month.split("-").map(Number);
    const first = new Date(y, m - 1, 1);
    const offset = (first.getDay() + 6) % 7; // lundi = 0
    const start = addDaysIso(month, -offset);
    return Array.from({ length: 42 }, (_, i) => addDaysIso(start, i));
  }, [month]);
  const { data } = useQuery({
    queryKey: ["calendar", days[0], days[41]],
    queryFn: () => api.get<CalendarSession[]>(`/missions/calendar?from=${days[0]}&to=${days[41]}`),
  });
  const shift = (n: number) => {
    const [y, m] = month.split("-").map(Number);
    const d = new Date(y, m - 1 + n, 1);
    setMonth(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-01`);
  };
  const label = new Date(`${month}T12:00:00`).toLocaleDateString("fr-FR", { month: "long", year: "numeric" });
  const today = todayIso();
  return (
    <div className="card">
      <div className="card-head">
        <div className="toolbar">
          <Button onClick={() => setMonth(monthStart(today))}>Aujourd'hui</Button>
          <button className="icon-btn" aria-label="Mois précédent" onClick={() => shift(-1)}>‹</button>
          <button className="icon-btn" aria-label="Mois suivant" onClick={() => shift(1)}>›</button>
          <h2 className="capitalize">{label}</h2>
        </div>
      </div>
      <div className="calendar">
        {["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"].map((d) => <div key={d} className="cal-head">{d}</div>)}
        {days.map((d) => (
          <div key={d} className={`cal-day ${d.slice(0, 7) !== month.slice(0, 7) ? "other" : ""} ${d === today ? "today" : ""}`}>
            <div className="cal-num">{Number(d.slice(8))}</div>
            {(data ?? []).filter((s) => s.date === d).map((s) => (
              <button key={s.id} className={`cal-event ${s.status === "REALISEE" ? "done" : s.status === "ANNULEE" ? "cancel" : ""}`} onClick={() => onOpen(s.mission_id)}
                title={`${s.title} - ${s.client_name} (${s.start_time}-${s.end_time})`}>
                {s.start_time} {s.title}
              </button>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

// Fenêtre de création : champs essentiels (intitulé, client, centre, formateur, tarif...), le reste dans la fiche.
function NewMissionModal({ onClose, onCreated }: { onClose: () => void; onCreated: (m: MissionDetail) => void }) {
  const qc = useQueryClient();
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people") });
  const { data: software } = useQuery({ queryKey: ["software"], queryFn: () => api.get<Software[]>("/software") });
  const { user } = useAuth();
  const [client, setClient] = useState<Option | null>(null);
  const { data: centers } = useQuery({ queryKey: ["training-centers"], queryFn: () => api.get<TrainingCenter[]>("/training-centers") });
  const { data: products } = useQuery({ queryKey: ["products"], queryFn: () => api.get<Product[]>("/products") });
  const [form, setForm] = useState({
    title: "", trainer_id: user?.id ?? null as number | null, software_id: null as number | null, modality: "PRESENTIEL", group_format: "INDIVIDUEL",
    pricing_mode: "HORAIRE", hourly_rate: 5000, daily_rate: 0, flat_fee: 0, planned_minutes: 0, os: "", center_id: null as number | null,
  });
  const [error, setError] = useState("");
  const create = useMutation({
    mutationFn: () => api.post<MissionDetail>("/missions", { ...form, client_id: client?.id ?? 0, billable: true }),
    onSuccess: (m) => { qc.invalidateQueries({ queryKey: ["missions"] }); onCreated(m); },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });
  const set = (k: string, v: unknown) => setForm({ ...form, [k]: v });
  return (
    <Modal open title="Nouvelle mission de formation" onClose={onClose}
      footer={<><Button onClick={onClose}>Annuler</Button><Button variant="primary" loading={create.isPending} onClick={() => create.mutate()}>Créer la mission</Button></>}>
      <div className="form-stack">
        <Alert>{error}</Alert>
        <TextField label="Intitulé de la formation" required value={form.title} onChange={(e) => set("title", e.target.value)} placeholder="ex. Utilisation de Revit : formation individuelle 7h" />
        <AsyncSelect label="Client (entreprise, organisme ou particulier)" value={client} search={searchContacts} onChange={setClient} />
        <SelectField label="Centre de formation" value={String(form.center_id ?? "")} onChange={(e) => set("center_id", e.target.value ? Number(e.target.value) : null)} options={centerOptions(centers ?? [], null, null)} />
        <div className="grid-2">
          <SelectField label="Formateur" value={String(form.trainer_id ?? "")} onChange={(e) => set("trainer_id", e.target.value ? Number(e.target.value) : null)}
            options={[{ value: "", label: "-" }, ...(people ?? []).map((p) => ({ value: String(p.id), label: p.display_name }))]} />
          <SelectField label="Logiciel" value={String(form.software_id ?? "")} onChange={(e) => set("software_id", e.target.value ? Number(e.target.value) : null)}
            options={[{ value: "", label: "-" }, ...(software ?? []).filter((s) => s.active).map((s) => ({ value: String(s.id), label: s.name }))]} />
          <SelectField label="Modalité" value={form.modality} onChange={(e) => set("modality", e.target.value)} options={Object.entries(MODALITY_LABELS).map(([value, label]) => ({ value, label }))} />
          <SelectField label="Format" value={form.group_format} onChange={(e) => set("group_format", e.target.value)} options={Object.entries(FORMAT_LABELS).map(([value, label]) => ({ value, label }))} />
          <SelectField label="OS" value={form.os} onChange={(e) => set("os", e.target.value)} options={[{ value: "", label: "-" }, { value: "PC", label: "PC" }, { value: "MAC", label: "Mac" }]} />
          <SelectField label="Tarification" value={form.pricing_mode} onChange={(e) => set("pricing_mode", e.target.value)} options={Object.entries(PRICING_LABELS).map(([value, label]) => ({ value, label }))} />
          <SelectField label="Produit (remplit le tarif)" value="" onChange={(e) => setForm(applyProductRate(form, products ?? [], e.target.value))} options={productOptions(products ?? [])} />
          {form.pricing_mode === "HORAIRE" ? <MoneyField label="Tarif horaire" value={form.hourly_rate} onChange={(v) => set("hourly_rate", v)} /> : null}
          {form.pricing_mode === "JOURNALIER" ? <MoneyField label="Tarif journalier" value={form.daily_rate} onChange={(v) => set("daily_rate", v)} /> : null}
          {form.pricing_mode === "FORFAIT" ? <MoneyField label="Forfait" value={form.flat_fee} onChange={(v) => set("flat_fee", v)} /> : null}
          <DurationField label="Durée prévue" value={form.planned_minutes} onChange={(v) => set("planned_minutes", v)} />
        </div>
      </div>
    </Modal>
  );
}
