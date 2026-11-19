import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import type { Page, Project } from "../../api/types";
import { MoneyField, Stat } from "../../components/Fields";
import { AsyncSelect, Option } from "../../components/Pickers";
import { Alert, Badge, Button, Card, Checkbox, ConfirmDialog, Loading, Modal, PageHeader, SearchInput, SelectField, TextAreaField, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { formatBytes, formatDate, formatMoney, todayIso } from "../../lib/format";
import { useDebounced, useUrlState } from "../../lib/hooks";
import { searchContacts } from "../../lib/search";

// Structures renvoyées par /depenses (miroir du serveur).
export interface Expense {
  id: number; date: string; label: string; category: string; supplier_id: number | null; supplier_name: string; supplier_display: string;
  amount_ht: number; vat_amount: number; amount_ttc: number; payment_method: string; status: "A_PAYER" | "PAYEE"; payment_date: string | null;
  project_id: number | null; project_name: string | null; mission_id: number | null; deductible: boolean; notes: string;
  receipt_name: string | null; receipt_size: number | null; created_at: string; updated_at: string;
}
interface ListResponse {
  items: Expense[]; total_ht: number; total_vat: number; total_ttc: number; to_pay: number;
  by_category: { category: string; count: number; amount_ht: number; amount_ttc: number }[];
  by_month: { month: string; amount_ttc: number }[]; truncated: boolean;
}

// Catégories et modes de paiement (mêmes codes que le serveur).
export const EXPENSE_CATEGORIES: Record<string, string> = {
  LOGICIEL: "Logiciels et abonnements", MATERIEL: "Matériel informatique", FOURNITURES: "Fournitures", DEPLACEMENT: "Déplacements", REPAS: "Repas",
  HEBERGEMENT: "Hébergement", TELECOM: "Téléphone et internet", ASSURANCE: "Assurances", COTISATIONS: "Cotisations et taxes", FORMATION: "Formation personnelle",
  HONORAIRES: "Honoraires (comptable, juridique)", MARKETING: "Communication et publicité", AUTRE: "Autre",
};
const METHODS: Record<string, string> = { CARTE: "Carte", VIREMENT: "Virement", PRELEVEMENT: "Prélèvement", CHEQUE: "Chèque", ESPECES: "Espèces" };
const MONTHS = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

// Formulaire d'une dépense : la TVA peut être saisie par taux (%) ou en montant.
interface Form {
  id?: number; date: string; label: string; category: string; supplier: Option | null; supplier_name: string; amount_ht: number; vat_rate: string; vat_amount: number;
  payment_method: string; status: "A_PAYER" | "PAYEE"; payment_date: string; project: Option | null; deductible: boolean; notes: string;
}
const emptyForm = (): Form => ({ date: todayIso(), label: "", category: "LOGICIEL", supplier: null, supplier_name: "", amount_ht: 0, vat_rate: "20", vat_amount: 0, payment_method: "CARTE", status: "PAYEE", payment_date: "", project: null, deductible: true, notes: "" });
const toForm = (e: Expense): Form => ({
  id: e.id, date: e.date, label: e.label, category: e.category, supplier: e.supplier_id ? { id: e.supplier_id, label: e.supplier_display } : null, supplier_name: e.supplier_name,
  amount_ht: e.amount_ht, vat_rate: e.amount_ht ? String(Math.round((e.vat_amount * 10000) / e.amount_ht) / 100).replace(".", ",") : "0", vat_amount: e.vat_amount,
  payment_method: e.payment_method, status: e.status, payment_date: e.payment_date ?? "", project: e.project_id ? { id: e.project_id, label: e.project_name ?? "" } : null, deductible: e.deductible, notes: e.notes,
});
// Montant de TVA depuis un taux en pourcentage ("20", "5,5").
const vatFromRate = (ht: number, rate: string) => { const r = Number(rate.replace(",", ".")); return Number.isFinite(r) ? Math.round((ht * r) / 100) : 0; };

// Écran Dépenses : filtres (URL), indicateurs, ventilation par catégorie et par mois, liste, saisie et justificatifs.
export function ExpensesPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const year = todayIso().slice(0, 4);
  const [f, setF] = useUrlState({ q: "", from: `${year}-01-01`, to: `${year}-12-31`, category: "", status: "" });
  const [search, setSearch] = useState(f.q);
  const dq = useDebounced(search, 300);
  useEffect(() => { setF({ q: dq }); }, [dq]); // eslint-disable-line react-hooks/exhaustive-deps
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v) params.set(k, v);
  const { data, isLoading } = useQuery({ queryKey: ["depenses", params.toString()], queryFn: () => api.get<ListResponse>(`/depenses?${params}`), placeholderData: (p) => p });
  const [edit, setEdit] = useState<Form | null>(null);
  const [toDelete, setToDelete] = useState<Expense | null>(null);
  const refresh = () => { qc.invalidateQueries({ queryKey: ["depenses"] }); qc.invalidateQueries({ queryKey: ["dashboard"] }); };
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");

  // Marquer payée / supprimer.
  const pay = useMutation({ mutationFn: (id: number) => api.post<Expense>(`/depenses/${id}/pay`, {}), onSuccess: () => { refresh(); toast("Dépense marquée payée"); }, onError });
  const remove = useMutation({ mutationFn: (id: number) => api.del(`/depenses/${id}`), onSuccess: () => { setToDelete(null); refresh(); toast("Dépense supprimée"); }, onError: (e) => { setToDelete(null); onError(e); } });

  const years = Array.from({ length: 6 }, (_, i) => String(Number(year) - i));
  const maxMonth = Math.max(1, ...(data?.by_month ?? []).map((m) => m.amount_ttc));

  return (
    <>
      <PageHeader title="Dépenses" actions={<><a className="btn" href={`/api/depenses/export.csv?${params}`} download>Exporter (CSV)</a><Button variant="primary" onClick={() => setEdit(emptyForm())}>Nouvelle dépense</Button></>}>
        <div className="toolbar">
          <SearchInput value={search} onChange={setSearch} placeholder="Libellé, fournisseur, note..." />
          <select aria-label="Année" value={years.includes(f.from.slice(0, 4)) && f.from.endsWith("-01-01") && f.to === `${f.from.slice(0, 4)}-12-31` ? f.from.slice(0, 4) : ""} onChange={(e) => e.target.value && setF({ from: `${e.target.value}-01-01`, to: `${e.target.value}-12-31` })}>
            <option value="">Période personnalisée</option>{years.map((y) => <option key={y} value={y}>{y}</option>)}
          </select>
          <input aria-label="Du" type="date" value={f.from} onChange={(e) => setF({ from: e.target.value })} />
          <input aria-label="Au" type="date" value={f.to} onChange={(e) => setF({ to: e.target.value })} />
          <select aria-label="Catégorie" value={f.category} onChange={(e) => setF({ category: e.target.value })}>
            <option value="">Toutes les catégories</option>{Object.entries(EXPENSE_CATEGORIES).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
          <select aria-label="Statut" value={f.status} onChange={(e) => setF({ status: e.target.value })}>
            <option value="">Tous les statuts</option><option value="A_PAYER">À payer</option><option value="PAYEE">Payées</option>
          </select>
        </div>
      </PageHeader>
      <div className="page-body wide form-stack">
        {isLoading || !data ? <Loading /> : (
          <>
            <div className="stats">
              <Stat label="Total TTC" value={formatMoney(data.total_ttc)} sub={`${data.items.length} dépense(s)`} />
              <Stat label="Total HT" value={formatMoney(data.total_ht)} />
              <Stat label="TVA" value={formatMoney(data.total_vat)} sub="récupérable si assujetti" />
              <Stat label="À payer" value={formatMoney(data.to_pay)} tone={data.to_pay > 0 ? "warning" : "success"} onClick={() => setF({ status: f.status === "A_PAYER" ? "" : "A_PAYER" })} />
            </div>
            <div className="grid-2 cards">
              <Card title="Par catégorie">
                {data.by_category.length === 0 ? <p className="muted">Aucune dépense sur la période.</p> : (
                  <table className="table compact">
                    <tbody>
                      {data.by_category.map((c) => (
                        <tr key={c.category} className={f.category === c.category ? "selected" : undefined} onClick={() => setF({ category: f.category === c.category ? "" : c.category })} style={{ cursor: "pointer" }}>
                          <td>{EXPENSE_CATEGORIES[c.category] ?? c.category} <span className="muted small">({c.count})</span></td>
                          <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(c.amount_ttc)}</td>
                          <td style={{ width: "40%" }}><div className="bar-track"><div className="bar-fill" style={{ width: `${Math.round((c.amount_ttc * 100) / Math.max(1, data.by_category[0].amount_ttc))}%` }} /></div></td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </Card>
              <Card title="Par mois">
                {data.by_month.length === 0 ? <p className="muted">Aucune dépense sur la période.</p> : (
                  <div className="bar-chart">
                    {data.by_month.map((m) => (
                      <div key={m.month} className="bar-col" title={`${m.month} : ${formatMoney(m.amount_ttc)}`}>
                        <div className="bar" style={{ height: `${Math.round((m.amount_ttc * 100) / maxMonth)}%` }} />
                        <span className="bar-label">{MONTHS[Number(m.month.slice(5, 7)) - 1]}{data.by_month.some((x) => x.month.slice(0, 4) !== m.month.slice(0, 4)) ? ` ${m.month.slice(2, 4)}` : ""}</span>
                      </div>
                    ))}
                  </div>
                )}
              </Card>
            </div>
            <Card>
              {data.truncated ? <Alert tone="warning">Liste limitée à 2000 lignes : affinez la période.</Alert> : null}
              <div className="table-wrap">
                <table className="table">
                  <thead><tr><th>Date</th><th>Libellé</th><th>Catégorie</th><th>Fournisseur</th><th>Mission</th><th style={{ textAlign: "right" }}>HT</th><th style={{ textAlign: "right" }}>TVA</th><th style={{ textAlign: "right" }}>TTC</th><th>Statut</th><th>Justificatif</th><th /></tr></thead>
                  <tbody>
                    {data.items.map((e) => (
                      <tr key={e.id}>
                        <td className="nowrap">{formatDate(e.date)}</td>
                        <td><a href="#" onClick={(ev) => { ev.preventDefault(); setEdit(toForm(e)); }}>{e.label}</a>{e.notes ? <div className="muted small">{e.notes}</div> : null}</td>
                        <td>{EXPENSE_CATEGORIES[e.category] ?? e.category}</td>
                        <td>{e.supplier_id ? <Link to={`/contacts/${e.supplier_id}`}>{e.supplier_display}</Link> : e.supplier_display}</td>
                        <td>{e.project_id ? (e.mission_id ? <Link to={`/missions/${e.mission_id}`}>{e.project_name}</Link> : <Link to={`/projets/${e.project_id}`}>{e.project_name}</Link>) : ""}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(e.amount_ht)}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{e.vat_amount ? formatMoney(e.vat_amount) : "-"}</td>
                        <td style={{ textAlign: "right" }} className="nowrap"><strong>{formatMoney(e.amount_ttc)}</strong></td>
                        <td>{e.status === "PAYEE" ? <Badge tone="success">Payée{e.payment_date ? ` le ${formatDate(e.payment_date)}` : ""}</Badge> : <Badge tone="warning">À payer</Badge>}{!e.deductible ? <div className="muted small">non déductible</div> : null}</td>
                        <td>{e.receipt_name ? <a href={`/api/depenses/${e.id}/receipt`} target="_blank" rel="noreferrer" title={e.receipt_name}>Voir{e.receipt_size ? ` (${formatBytes(e.receipt_size)})` : ""}</a> : <span className="muted small">aucun</span>}</td>
                        <td className="nowrap">
                          {e.status === "A_PAYER" ? <Button variant="ghost" onClick={() => pay.mutate(e.id)}>Payée</Button> : null}
                          <Button variant="ghost" className="text-danger" onClick={() => setToDelete(e)}>Supprimer</Button>
                        </td>
                      </tr>
                    ))}
                    {data.items.length === 0 ? <tr><td colSpan={11} className="table-empty">Aucune dépense avec ces critères.</td></tr> : null}
                  </tbody>
                </table>
              </div>
            </Card>
          </>
        )}
      </div>
      {edit ? <ExpenseModal form={edit} onClose={() => setEdit(null)} onSaved={() => { setEdit(null); refresh(); }} /> : null}
      <ConfirmDialog open={!!toDelete} title="Supprimer la dépense" danger confirmLabel="Supprimer" onCancel={() => setToDelete(null)} onConfirm={() => toDelete && remove.mutate(toDelete.id)}
        message={toDelete ? `Supprimer "${toDelete.label}" (${formatMoney(toDelete.amount_ttc)}) et son justificatif ? Cette action est journalisée.` : ""} />
    </>
  );
}

// Fenêtre de saisie / modification d'une dépense, avec envoi du justificatif.
function ExpenseModal({ form: initial, onClose, onSaved }: { form: Form; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [form, setForm] = useState<Form>(initial);
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm((f) => ({ ...f, [k]: v }));
  // Projets ouverts proposés pour le rattachement.
  const { data: projects } = useQuery({ queryKey: ["projects", "depenses"], queryFn: () => api.get<Page<Project>>("/projects?per_page=500") });
  const searchProjects = async (q: string): Promise<Option[]> => (projects?.items ?? []).filter((p) => `${p.reference} ${p.name} ${p.client_name}`.toLowerCase().includes(q.toLowerCase())).slice(0, 15).map((p) => ({ id: p.id, label: `${p.reference} ${p.name} (${p.client_name})` }));

  const body = () => ({
    date: form.date, label: form.label, category: form.category, supplier_id: form.supplier?.id ?? null, supplier_name: form.supplier ? "" : form.supplier_name,
    amount_ht: form.amount_ht, vat_amount: form.vat_amount, payment_method: form.payment_method, status: form.status, payment_date: form.payment_date || null,
    project_id: form.project?.id ?? null, deductible: form.deductible, notes: form.notes,
  });
  // Enregistre (POST ou PUT) puis envoie le justificatif choisi s'il y en a un.
  const save = useMutation({
    mutationFn: async () => {
      const saved = form.id ? await api.put<Expense>(`/depenses/${form.id}`, body()) : await api.post<Expense>("/depenses", body());
      const file = fileRef.current?.files?.[0];
      if (file) {
        const fd = new FormData();
        fd.append("file", file);
        await api.post<Expense>(`/depenses/${saved.id}/receipt`, fd);
      }
      return saved;
    },
    onSuccess: () => { toast("Dépense enregistrée"); onSaved(); },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });

  return (
    <Modal open title={form.id ? "Modifier la dépense" : "Nouvelle dépense"} onClose={onClose}
      footer={<><Button onClick={onClose}>Annuler</Button><Button variant="primary" loading={save.isPending} onClick={() => save.mutate()}>Enregistrer</Button></>}>
      <div className="form-stack">
        <Alert>{error}</Alert>
        <div className="grid-2">
          <TextField label="Date" type="date" required value={form.date} onChange={(e) => set("date", e.target.value)} />
          <SelectField label="Catégorie" value={form.category} onChange={(e) => set("category", e.target.value)} options={Object.entries(EXPENSE_CATEGORIES).map(([value, label]) => ({ value, label }))} />
        </div>
        <TextField label="Libellé" required value={form.label} onChange={(e) => set("label", e.target.value)} placeholder="ex. Abonnement AutoCAD, train Bordeaux-Paris..." />
        <div className="grid-2">
          <AsyncSelect label="Fournisseur (contact)" value={form.supplier} search={(q) => searchContacts(q, {})} onChange={(o) => set("supplier", o)} />
          <TextField label="ou fournisseur libre" value={form.supplier_name} disabled={!!form.supplier} onChange={(e) => set("supplier_name", e.target.value)} placeholder="ex. Autodesk, SNCF" />
        </div>
        <div className="grid-3">
          <MoneyField label="Montant HT" value={form.amount_ht} onChange={(v) => setForm((f) => ({ ...f, amount_ht: v, vat_amount: vatFromRate(v, f.vat_rate) }))} />
          <TextField label="Taux de TVA (%)" value={form.vat_rate} onChange={(e) => setForm((f) => ({ ...f, vat_rate: e.target.value, vat_amount: vatFromRate(f.amount_ht, e.target.value) }))} hint="0 si pas de TVA" />
          <MoneyField label="Montant de TVA" value={form.vat_amount} onChange={(v) => set("vat_amount", v)} />
        </div>
        <p className="muted small" style={{ textAlign: "right" }}>TTC : <strong>{formatMoney(form.amount_ht + form.vat_amount)}</strong></p>
        <div className="grid-3">
          <SelectField label="Mode de paiement" value={form.payment_method} onChange={(e) => set("payment_method", e.target.value)} options={Object.entries(METHODS).map(([value, label]) => ({ value, label }))} />
          <SelectField label="Statut" value={form.status} onChange={(e) => set("status", e.target.value as Form["status"])} options={[{ value: "PAYEE", label: "Payée" }, { value: "A_PAYER", label: "À payer" }]} />
          {form.status === "PAYEE" ? <TextField label="Date de paiement" type="date" value={form.payment_date} onChange={(e) => set("payment_date", e.target.value)} hint="Vide = date de la dépense" /> : <div />}
        </div>
        <AsyncSelect label="Mission ou projet concerné (facultatif)" value={form.project} search={searchProjects} onChange={(o) => set("project", o)} />
        <div className="field">
          <label>Justificatif (PDF, photo...)</label>
          <input ref={fileRef} type="file" accept=".pdf,image/*" onChange={(e) => setReceipt(e.target.files?.[0]?.name ?? null)} />
          {initial.id && !receipt ? <div className="muted small">Un nouveau fichier remplacera le justificatif actuel.</div> : null}
        </div>
        <Checkbox label="Dépense déductible" checked={form.deductible} onChange={(e) => set("deductible", e.target.checked)} />
        <TextAreaField label="Notes" value={form.notes} onChange={(e) => set("notes", e.target.value)} />
      </div>
    </Modal>
  );
}
