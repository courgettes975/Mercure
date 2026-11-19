import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import type { Person, TimesheetChoice, TimesheetRow } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { DurationField } from "../../components/Fields";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Checkbox, Loading, Modal, PageHeader, SearchInput, SelectField, TextField } from "../../components/ui";
import { addDaysIso, durationInput, formatDate, formatMinutes, formatMoney, parseDuration, todayIso } from "../../lib/format";
import { useDebounced, useUrlState } from "../../lib/hooks";

// Réponse de /timesheets : lignes + totaux.
interface ListResponse {
  items: TimesheetRow[];
  totals: { minutes: number; billable_minutes: number; billable_amount: number; uninvoiced_amount: number };
  truncated: boolean;
}

// Lundi de la semaine d'une date ISO.
function weekStart(iso: string) {
  const [y, m, d] = iso.split("-").map(Number);
  const dt = new Date(y, m - 1, d);
  return addDaysIso(iso, -((dt.getDay() + 6) % 7));
}
// Premier et dernier jour du mois d'une date ISO.
function monthBounds(iso: string): [string, string] {
  const [y, m] = iso.split("-").map(Number);
  const last = new Date(y, m, 0).getDate();
  return [`${iso.slice(0, 7)}-01`, `${iso.slice(0, 7)}-${String(last).padStart(2, "0")}`];
}
// "lun. 18/11".
function dayLabel(iso: string) {
  const d = new Date(`${iso}T12:00:00`);
  return { wd: d.toLocaleDateString("fr-FR", { weekday: "short" }), dm: d.toLocaleDateString("fr-FR", { day: "numeric", month: "short" }), weekend: d.getDay() === 0 || d.getDay() === 6 };
}

// Écran Feuilles de temps : navigation par semaine / mois, vue grille (saisie rapide) ou liste.
export function TimesheetsPage() {
  const { user } = useAuth();
  const manager = hasRole(user, "ADMIN", "GESTIONNAIRE");
  const [f, setF] = useUrlState({ vue: "grille", periode: "semaine", date: todayIso(), user_id: "", q: "", group: "projet", billable: "", invoiced: "" });
  const [from, to] = f.periode === "mois" ? monthBounds(f.date) : [weekStart(f.date), addDaysIso(weekStart(f.date), 6)];
  const viewedUser = manager && f.user_id ? Number(f.user_id) : user!.id;
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people"), enabled: manager });

  // Période précédente / suivante.
  const shift = (n: number) => {
    if (f.periode === "mois") {
      const [y, m] = f.date.split("-").map(Number);
      const d = new Date(y, m - 1 + n, 1);
      setF({ date: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-01` });
    } else setF({ date: addDaysIso(f.date, 7 * n) });
  };
  const title = f.periode === "mois"
    ? new Date(`${from}T12:00:00`).toLocaleDateString("fr-FR", { month: "long", year: "numeric" })
    : `Semaine du ${formatDate(from)} au ${formatDate(to)}`;

  return (
    <>
      <PageHeader title="Feuilles de temps">
        <div className="segmented small-seg">
          {[["grille", "Grille"], ["liste", "Liste"]].map(([k, l]) => (
            <label key={k} className={f.vue === k ? "active" : ""}><input type="radio" name="vue" checked={f.vue === k} onChange={() => setF({ vue: k })} />{l}</label>
          ))}
        </div>
      </PageHeader>
      <div className="filter-bar">
        <Button onClick={() => setF({ date: todayIso() })}>Aujourd'hui</Button>
        <button className="icon-btn" aria-label="Précédent" onClick={() => shift(-1)}>‹</button>
        <select aria-label="Période" value={f.periode} onChange={(e) => setF({ periode: e.target.value })}>
          <option value="semaine">Semaine</option>
          <option value="mois">Mois</option>
        </select>
        <button className="icon-btn" aria-label="Suivant" onClick={() => shift(1)}>›</button>
        <strong className="capitalize">{title}</strong>
        {manager ? (
          <select aria-label="Utilisateur" value={f.user_id} onChange={(e) => setF({ user_id: e.target.value })}>
            <option value="">{f.vue === "liste" ? "Tous les intervenants" : "Mes temps"}</option>
            {(people ?? []).map((p) => <option key={p.id} value={p.id}>{p.display_name}</option>)}
          </select>
        ) : null}
      </div>
      <div className="page-body wide">
        {f.vue === "grille" ? (
          <Grid from={from} to={to} userId={viewedUser} editable={viewedUser === user!.id} />
        ) : (
          <ListView from={from} to={to} f={f} setF={setF} manager={manager} />
        )}
      </div>
    </>
  );
}

// Grille projet/tâche x jours : une case = total du jour, modifiable si la ligne n'est pas verrouillée.
function Grid({ from, to, userId, editable }: { from: string; to: string; userId: number; editable: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const days = useMemo(() => {
    const out: string[] = [];
    for (let d = from; d <= to; d = addDaysIso(d, 1)) out.push(d);
    return out;
  }, [from, to]);
  const key = ["timesheets", "grid", from, to, userId];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api.get<ListResponse>(`/timesheets?from=${from}&to=${to}&user_id=${userId}&limit=5000`) });
  const { data: choices } = useQuery({ queryKey: ["timesheet-choices"], queryFn: () => api.get<TimesheetChoice[]>("/timesheets/choices") });
  const [extraRows, setExtraRows] = useState<{ project_id: number; task_id: number | null; label: string }[]>([]);
  const [adding, setAdding] = useState(false);
  const [pick, setPick] = useState("");

  // Enregistre le total d'une case (le serveur crée, met à jour ou supprime la ligne).
  const setCell = useMutation({
    mutationFn: (c: { date: string; project_id: number; task_id: number | null; minutes: number }) => api.put("/timesheets/cell", c),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["timesheets"] }),
    onError: (e) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"),
  });

  if (isLoading || !data) return <Loading />;
  // Regroupe les temps par projet/tâche ; une case est verrouillée si elle contient un temps de session ou facturé.
  type Row = { key: string; project_id: number; task_id: number | null; label: string; client: string; cells: Record<string, number>; locked: Record<string, boolean> };
  const rows = new Map<string, Row>();
  for (const t of data.items) {
    const k = `${t.project_id}:${t.task_id ?? ""}`;
    if (!rows.has(k)) rows.set(k, { key: k, project_id: t.project_id, task_id: t.task_id, label: t.task_name ? `${t.project_name} | ${t.task_name}` : t.project_name, client: t.client_name, cells: {}, locked: {} });
    const r = rows.get(k)!;
    r.cells[t.date] = (r.cells[t.date] ?? 0) + t.duration_minutes;
    if (t.session_id || t.invoice_line_id) r.locked[t.date] = true;
  }
  // Lignes ajoutées manuellement (projet choisi dans le menu) pas encore remplies.
  for (const e of extraRows) {
    const k = `${e.project_id}:${e.task_id ?? ""}`;
    if (!rows.has(k)) {
      const c = (choices ?? []).find((x) => x.project_id === e.project_id);
      rows.set(k, { key: k, project_id: e.project_id, task_id: e.task_id, label: e.label, client: c?.client_name ?? "", cells: {}, locked: {} });
    }
  }
  const list = [...rows.values()].sort((a, b) => (a.client + a.label).localeCompare(b.client + b.label, "fr"));
  const dayTotal = (d: string) => list.reduce((s, r) => s + (r.cells[d] ?? 0), 0);
  const rowTotal = (r: Row) => Object.values(r.cells).reduce((s, v) => s + v, 0);
  const month = days.length > 7;

  return (
    <div className="form-stack">
      <div className="stats">
        <div className="stat"><span className="stat-value">{formatMinutes(data.totals.minutes)}</span><span className="stat-label">travaillées</span></div>
        <div className="stat"><span className="stat-value">{formatMinutes(data.totals.billable_minutes)}</span><span className="stat-label">facturables</span></div>
        <div className="stat"><span className="stat-value">{formatMoney(data.totals.billable_amount)}</span><span className="stat-label">valeur facturable</span></div>
      </div>
      <div className="table-wrap">
        <table className={`table grid-table ${month ? "month" : ""}`}>
          <thead>
            <tr>
              <th className="grid-label">Projet | Tâche</th>
              {days.map((d) => { const l = dayLabel(d); return <th key={d} className={`grid-day ${l.weekend ? "weekend" : ""} ${d === todayIso() ? "today" : ""}`}>{l.wd}<br />{l.dm}</th>; })}
              <th className="grid-total">Total</th>
            </tr>
          </thead>
          <tbody>
            {list.map((r) => (
              <tr key={r.key}>
                <td className="grid-label"><span className="muted small">{r.client}</span><br />{r.label}</td>
                {days.map((d) => (
                  <td key={d} className={`grid-cell ${dayLabel(d).weekend ? "weekend" : ""} ${d === todayIso() ? "today" : ""}`}>
                    {editable && !r.locked[d] ? (
                      <CellInput minutes={r.cells[d] ?? 0} onSave={(m) => setCell.mutate({ date: d, project_id: r.project_id, task_id: r.task_id, minutes: m })} />
                    ) : (
                      <span className={r.cells[d] ? "" : "muted"} title={r.locked[d] ? "Temps issu d'une session ou facturé : modifiable depuis la mission" : undefined}>
                        {r.cells[d] ? formatMinutes(r.cells[d]) : "0h"}{r.locked[d] ? " 🔒" : ""}
                      </span>
                    )}
                  </td>
                ))}
                <td className="grid-total">{formatMinutes(rowTotal(r))}</td>
              </tr>
            ))}
            {editable ? (
              <tr>
                <td colSpan={days.length + 2}>
                  {adding ? (
                    <span className="toolbar">
                      <select aria-label="Projet et tâche" value={pick} onChange={(e) => setPick(e.target.value)}>
                        <option value="">Choisir un projet / une tâche...</option>
                        {(choices ?? []).map((c) => (
                          <option key={`${c.project_id}:${c.task_id ?? ""}`} value={`${c.project_id}:${c.task_id ?? ""}`}>
                            {c.client_name} | {c.project_name}{c.task_name ? ` | ${c.task_name}` : ""}
                          </option>
                        ))}
                        {(choices ?? []).filter((c) => c.task_id).map((c) => c.project_id).filter((v, i, a) => a.indexOf(v) === i).map((pid) => {
                          const c = choices!.find((x) => x.project_id === pid)!;
                          return <option key={`${pid}:`} value={`${pid}:`}>{c.client_name} | {c.project_name} (sans tâche)</option>;
                        })}
                      </select>
                      <Button variant="primary" disabled={!pick} onClick={() => {
                        const [p, t] = pick.split(":");
                        const c = choices!.find((x) => String(x.project_id) === p && String(x.task_id ?? "") === t);
                        setExtraRows([...extraRows, { project_id: Number(p), task_id: t ? Number(t) : null, label: c?.task_name ? `${c.project_name} | ${c.task_name}` : c?.project_name ?? "" }]);
                        setAdding(false);
                        setPick("");
                      }}>Ajouter</Button>
                      <Button onClick={() => setAdding(false)}>Annuler</Button>
                    </span>
                  ) : (
                    <button className="link-btn" onClick={() => setAdding(true)}>+ Ajouter une ligne</button>
                  )}
                </td>
              </tr>
            ) : null}
          </tbody>
          <tfoot>
            <tr>
              <td className="grid-label"><strong>Total</strong></td>
              {days.map((d) => <td key={d} className={`grid-cell ${dayLabel(d).weekend ? "weekend" : ""}`}><strong>{dayTotal(d) ? formatMinutes(dayTotal(d)) : ""}</strong></td>)}
              <td className="grid-total"><strong>{formatMinutes(data.totals.minutes)}</strong></td>
            </tr>
          </tfoot>
        </table>
      </div>
      {editable ? <p className="muted small">Saisie : 7, 7,5, 7h30 ou 45min. Les temps des sessions de formation (🔒) se modifient depuis la mission.</p> : <p className="muted small">Consultation des temps d'un autre intervenant (lecture seule).</p>}
    </div>
  );
}

// Saisie d'une case : texte libre ("7", "7h30") converti en minutes à la sortie.
function CellInput({ minutes, onSave }: { minutes: number; onSave: (m: number) => void }) {
  const [text, setText] = useState<string | null>(null);
  const shown = text ?? (minutes ? durationInput(minutes) : "");
  return (
    <input
      className={`cell-input ${minutes ? "filled" : ""}`}
      aria-label="Durée"
      value={shown}
      placeholder="0"
      onChange={(e) => setText(e.target.value)}
      onFocus={(e) => e.target.select()}
      onBlur={() => {
        if (text === null) return;
        const v = parseDuration(text);
        setText(null);
        if (v !== null && v !== minutes) onSave(v);
      }}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}

// Regroupements de la vue liste.
type Group = "aucun" | "utilisateur" | "client" | "centre" | "projet" | "tache" | "date";

// Vue liste : filtres, regroupement, totaux, sélection pour validation, édition / duplication / suppression.
function ListView({ from, to, f, setF, manager }: { from: string; to: string; f: Record<string, string>; setF: (p: Record<string, string>) => void; manager: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [text, setText] = useState(f.q);
  const q = useDebounced(text, 250);
  const params = new URLSearchParams({ from, to, limit: "5000" });
  if (f.user_id) params.set("user_id", f.user_id);
  if (q) params.set("q", q);
  if (f.billable) params.set("billable", f.billable);
  if (f.invoiced) params.set("invoiced", f.invoiced);
  const { data, isLoading } = useQuery({ queryKey: ["timesheets", "list", params.toString()], queryFn: () => api.get<ListResponse>(`/timesheets?${params}`), placeholderData: (p) => p });
  const [selected, setSelected] = useState<number[]>([]);
  const [edit, setEdit] = useState<Partial<TimesheetRow> | null>(null);
  const refresh = () => qc.invalidateQueries({ queryKey: ["timesheets"] });
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");
  // Actions de la liste.
  const validate = useMutation({ mutationFn: (status: string) => api.post<{ updated: number }>("/timesheets/validate", { ids: selected, status }), onSuccess: (r) => { toast(`${r.updated} ligne(s) mise(s) à jour`); setSelected([]); refresh(); }, onError });
  const del = useMutation({ mutationFn: (id: number) => api.del(`/timesheets/${id}`), onSuccess: () => { toast("Ligne supprimée"); refresh(); }, onError });
  const dup = useMutation({ mutationFn: (id: number) => api.post(`/timesheets/${id}/duplicate`, {}), onSuccess: () => { toast("Ligne dupliquée"); refresh(); }, onError });

  const group = f.group as Group;
  const groups = useMemo(() => {
    const items = data?.items ?? [];
    const keyOf = (t: TimesheetRow) => group === "utilisateur" ? t.user_name : group === "client" ? t.client_name : group === "centre" ? (t.center_name ?? "Sans centre (client direct)") : group === "projet" ? `${t.client_name} | ${t.project_name}` : group === "tache" ? `${t.project_name} | ${t.task_name ?? "sans tâche"}` : group === "date" ? t.date : "";
    const map = new Map<string, TimesheetRow[]>();
    for (const t of items) map.set(keyOf(t), [...(map.get(keyOf(t)) ?? []), t]);
    return [...map.entries()].sort((a, b) => (group === "date" ? b[0].localeCompare(a[0]) : a[0].localeCompare(b[0], "fr")));
  }, [data, group]);

  const locked = (t: TimesheetRow) => !!t.invoice_line_id || !!t.session_id;
  return (
    <div className="form-stack">
      <div className="toolbar">
        <SearchInput value={text} onChange={setText} placeholder="Description, client, projet..." />
        <select aria-label="Regrouper" value={f.group} onChange={(e) => setF({ group: e.target.value })}>
          <option value="aucun">Sans regroupement</option>
          <option value="utilisateur">Par intervenant</option>
          <option value="client">Par client</option>
          <option value="centre">Par centre de formation</option>
          <option value="projet">Par projet</option>
          <option value="tache">Par tâche</option>
          <option value="date">Par date</option>
        </select>
        <select aria-label="Facturable" value={f.billable} onChange={(e) => setF({ billable: e.target.value })}>
          <option value="">Facturables et non facturables</option>
          <option value="true">Facturables</option>
          <option value="false">Non facturables</option>
        </select>
        <select aria-label="Facturé" value={f.invoiced} onChange={(e) => setF({ invoiced: e.target.value })}>
          <option value="">Facturés ou non</option>
          <option value="non">Non facturés</option>
          <option value="oui">Facturés</option>
        </select>
        <Button variant="primary" onClick={() => setEdit({ date: todayIso(), duration_minutes: 0, description: "", billable: true })}>Ajouter</Button>
        {manager ? <Link className="btn btn-ghost" to="/facturation/mensuelle">Facturer le mois par centre</Link> : null}
        {manager && selected.length ? (
          <>
            <Button onClick={() => validate.mutate("VALIDE")}>Valider ({selected.length})</Button>
            <Button variant="ghost" onClick={() => validate.mutate("SAISI")}>Dévalider</Button>
          </>
        ) : null}
      </div>
      {data ? (
        <div className="stats">
          <div className="stat"><span className="stat-label">Total heures</span><span className="stat-value">{formatMinutes(data.totals.minutes)}</span></div>
          <div className="stat"><span className="stat-label">Heures facturables</span><span className="stat-value">{formatMinutes(data.totals.billable_minutes)}</span></div>
          <div className="stat"><span className="stat-label">Valeur facturable</span><span className="stat-value">{formatMoney(data.totals.billable_amount)}</span></div>
          <div className="stat"><span className="stat-label">Non facturé</span><span className="stat-value">{formatMoney(data.totals.uninvoiced_amount)}</span></div>
        </div>
      ) : null}
      {isLoading || !data ? <Loading /> : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                {manager ? <th style={{ width: 30 }}><input type="checkbox" aria-label="Tout sélectionner" checked={selected.length > 0 && selected.length === data.items.filter((t) => !locked(t)).length}
                  onChange={(e) => setSelected(e.target.checked ? data.items.filter((t) => !locked(t)).map((t) => t.id) : [])} /></th> : null}
                <th>Date</th><th>Intervenant</th><th>Client</th><th>Projet</th><th>Tâche</th><th>Description</th>
                <th style={{ textAlign: "right" }}>Durée</th><th style={{ textAlign: "right" }}>Taux</th><th style={{ textAlign: "right" }}>Montant</th><th>Statut</th><th />
              </tr>
            </thead>
            <tbody>
              {groups.map(([g, items]) => (
                <Fragment key={g || "all"}>
                  {group !== "aucun" ? (
                    <tr className="group-row">
                      <td colSpan={manager ? 7 : 6}><strong>{group === "date" ? formatDate(g) : g}</strong> <span className="muted">({items.length})</span></td>
                      <td style={{ textAlign: "right" }}><strong>{formatMinutes(items.reduce((s, t) => s + t.duration_minutes, 0))}</strong></td>
                      <td />
                      <td style={{ textAlign: "right" }}><strong>{formatMoney(items.filter((t) => t.billable).reduce((s, t) => s + t.amount, 0))}</strong></td>
                      <td colSpan={2} />
                    </tr>
                  ) : null}
                  {items.map((t) => (
                    <tr key={t.id}>
                      {manager ? <td><input type="checkbox" aria-label="Sélectionner" disabled={locked(t)} checked={selected.includes(t.id)} onChange={(e) => setSelected(e.target.checked ? [...selected, t.id] : selected.filter((x) => x !== t.id))} /></td> : null}
                      <td className="nowrap">{formatDate(t.date)}</td>
                      <td>{t.user_name}</td>
                      <td>{t.client_name}{t.center_name ? <div className="muted small">{t.center_name}</div> : null}</td>
                      <td>{t.mission_id ? <Link to={`/missions/${t.mission_id}`}>{t.project_name}</Link> : <Link to={`/projets/${t.project_id}`}>{t.project_name}</Link>}</td>
                      <td>{t.task_name ?? ""}</td>
                      <td>{t.description}</td>
                      <td style={{ textAlign: "right" }} className="nowrap">{formatMinutes(t.duration_minutes)}</td>
                      <td style={{ textAlign: "right" }} className="nowrap">{t.billable ? `${formatMoney(t.hourly_rate)}/h` : ""}</td>
                      <td style={{ textAlign: "right" }} className="nowrap">{t.billable ? formatMoney(t.amount) : <span className="muted">non fact.</span>}</td>
                      <td>
                        {t.invoice_id ? <Link to={`/facturation/${t.invoice_id}`}><Badge tone="success">{t.invoice_number ?? "Brouillon"}</Badge></Link>
                          : t.status === "VALIDE" ? <Badge tone="info">Validé</Badge> : <Badge>Saisi</Badge>}
                        {t.session_id ? <Badge tone="neutral">Session</Badge> : null}
                      </td>
                      <td className="row-actions">
                        {!locked(t) ? <>
                          <Button variant="ghost" onClick={() => setEdit(t)}>Modifier</Button>
                          <Button variant="ghost" onClick={() => dup.mutate(t.id)}>Dupliquer</Button>
                          <Button variant="ghost" className="text-danger" onClick={() => del.mutate(t.id)}>Supprimer</Button>
                        </> : null}
                      </td>
                    </tr>
                  ))}
                </Fragment>
              ))}
              {data.items.length === 0 ? <tr><td colSpan={12} className="table-empty">Aucun temps sur la période</td></tr> : null}
            </tbody>
          </table>
        </div>
      )}
      {edit ? <TimesheetModal row={edit} manager={manager} onClose={() => setEdit(null)} onSaved={() => { setEdit(null); refresh(); toast("Temps enregistré"); }} /> : null}
    </div>
  );
}

// Fenêtre d'ajout / modification d'une ligne (projet, tâche, date, durée, description, facturable).
function TimesheetModal({ row, manager, onClose, onSaved }: { row: Partial<TimesheetRow>; manager: boolean; onClose: () => void; onSaved: () => void }) {
  const { data: choices } = useQuery({ queryKey: ["timesheet-choices"], queryFn: () => api.get<TimesheetChoice[]>("/timesheets/choices") });
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people"), enabled: manager });
  const { user } = useAuth();
  const [form, setForm] = useState({
    date: row.date ?? todayIso(),
    pick: row.project_id ? `${row.project_id}:${row.task_id ?? ""}` : "",
    description: row.description ?? "",
    duration_minutes: row.duration_minutes ?? 0,
    billable: row.billable ?? true,
    user_id: row.user_id ?? user!.id,
  });
  const [error, setError] = useState("");
  const options = useMemo(() => {
    const out: { value: string; label: string }[] = [{ value: "", label: "Choisir..." }];
    const seen = new Set<number>();
    for (const c of choices ?? []) {
      if (!seen.has(c.project_id)) {
        seen.add(c.project_id);
        out.push({ value: `${c.project_id}:`, label: `${c.client_name} | ${c.project_name}` });
      }
      if (c.task_id) out.push({ value: `${c.project_id}:${c.task_id}`, label: `${c.client_name} | ${c.project_name} | ${c.task_name}` });
    }
    if (row.project_id && !out.some((o) => o.value === form.pick)) out.push({ value: form.pick, label: `${row.client_name} | ${row.project_name}${row.task_name ? ` | ${row.task_name}` : ""}` });
    return out;
  }, [choices, row, form.pick]);
  const save = async () => {
    const [p, t] = form.pick.split(":");
    if (!p) return setError("Choisissez un projet");
    const body = { date: form.date, project_id: Number(p), task_id: t ? Number(t) : null, description: form.description, duration_minutes: form.duration_minutes, billable: form.billable, user_id: form.user_id };
    try {
      if (row.id) await api.put(`/timesheets/${row.id}`, body);
      else await api.post("/timesheets", body);
      onSaved();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    }
  };
  return (
    <Modal open title={row.id ? "Modifier le temps" : "Nouveau temps"} onClose={onClose}
      footer={<><Button onClick={onClose}>Annuler</Button><Button variant="primary" onClick={save}>Enregistrer</Button></>}>
      <div className="form-stack">
        <Alert>{error}</Alert>
        <div className="grid-2">
          <TextField label="Date" type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} />
          <DurationField label="Durée" value={form.duration_minutes} onChange={(v) => setForm({ ...form, duration_minutes: v })} />
        </div>
        <SelectField label="Projet / tâche" value={form.pick} onChange={(e) => setForm({ ...form, pick: e.target.value })} options={options} />
        <TextField label="Description" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
        {manager ? (
          <SelectField label="Intervenant" value={String(form.user_id)} onChange={(e) => setForm({ ...form, user_id: Number(e.target.value) })}
            options={(people ?? []).map((p) => ({ value: String(p.id), label: p.display_name }))} />
        ) : null}
        <Checkbox label="Facturable" checked={form.billable} onChange={(e) => setForm({ ...form, billable: e.target.checked })} />
      </div>
    </Modal>
  );
}
