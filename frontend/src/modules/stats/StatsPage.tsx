import { useQuery } from "@tanstack/react-query";
import { api } from "../../api/client";
import type { Person, Software, TrainingCenter } from "../../api/types";
import { AsyncSelect, Option } from "../../components/Pickers";
import { Stat } from "../../components/Fields";
import { Button, Card, Loading, PageHeader, SelectField, TextField } from "../../components/ui";
import { formatMinutes, formatMoney, todayIso } from "../../lib/format";
import { useUrlState } from "../../lib/hooks";
import { searchContacts } from "../../lib/search";
import { useState } from "react";

// Structures renvoyées par /stats.
interface StatRow {
  key: string; label: string; minutes: number; billable_minutes: number; timesheet_amount: number; missions: number; participants: number;
  sessions: number; session_minutes: number; invoices: number; invoiced_ht: number; invoiced_time: number; invoiced_expenses: number; invoiced_flat: number; paid: number; expenses_ttc: number;
}
interface StatsReport { from: string; to: string; group_by: string; rows: StatRow[]; total: StatRow }

// Regroupements proposés.
const GROUPS: { value: string; label: string }[] = [
  { value: "centre", label: "Par centre de formation" }, { value: "client", label: "Par client" }, { value: "formateur", label: "Par formateur" },
  { value: "logiciel", label: "Par logiciel" }, { value: "modalite", label: "Par modalité (présentiel / distanciel)" }, { value: "format", label: "Par format (individuel / groupe)" },
  { value: "os", label: "Par OS (PC / Mac)" }, { value: "mois", label: "Par mois" }, { value: "annee", label: "Par année" }, { value: "total", label: "Total seul" },
];

// Statistiques : période, regroupement, filtres (URL), indicateurs, tableau et exports CSV.
export function StatsPage() {
  const year = todayIso().slice(0, 4);
  const [f, setF] = useUrlState({ from: `${year}-01-01`, to: `${year}-12-31`, group_by: "centre", center_id: "", client_id: "", trainer_id: "", software_id: "", modality: "", group_format: "", os: "" });
  const [client, setClient] = useState<Option | null>(null);
  const { data: centers } = useQuery({ queryKey: ["training-centers"], queryFn: () => api.get<TrainingCenter[]>("/training-centers") });
  const { data: people } = useQuery({ queryKey: ["people"], queryFn: () => api.get<Person[]>("/people") });
  const { data: software } = useQuery({ queryKey: ["software"], queryFn: () => api.get<Software[]>("/software") });
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v) params.set(k, v);
  const { data, isLoading, error } = useQuery({ queryKey: ["stats", params.toString()], queryFn: () => api.get<StatsReport>(`/stats?${params}`), placeholderData: (p) => p });

  // Raccourci "année" : du 1er janvier au 31 décembre.
  const setYear = (y: string) => setF({ from: `${y}-01-01`, to: `${y}-12-31` });
  // Liens de téléchargement des CSV (mêmes paramètres que le tableau).
  const exportUrl = (dataset: string) => `/api/stats/export.csv?${params}&dataset=${dataset}`;
  const years = Array.from({ length: 6 }, (_, i) => String(Number(year) - i));
  const t = data?.total;

  return (
    <>
      <PageHeader title="Statistiques" actions={<a className="btn btn-primary" href={exportUrl("synthese")} download>Exporter la synthèse (CSV)</a>}>
        <p className="muted">Heures travaillées, formations, stagiaires, montants facturés et encaissés sur une période, regroupés et filtrés selon vos critères. Les fichiers CSV s'ouvrent dans Excel.</p>
      </PageHeader>
      <div className="page-body wide form-stack">
        <Card title="Critères">
          <div className="grid-3">
            <SelectField label="Année" value={years.includes(f.from.slice(0, 4)) && f.from.endsWith("-01-01") && f.to === `${f.from.slice(0, 4)}-12-31` ? f.from.slice(0, 4) : ""} onChange={(e) => e.target.value && setYear(e.target.value)}
              options={[{ value: "", label: "Période personnalisée" }, ...years.map((y) => ({ value: y, label: y }))]} />
            <TextField label="Du" type="date" value={f.from} onChange={(e) => setF({ from: e.target.value })} />
            <TextField label="Au" type="date" value={f.to} onChange={(e) => setF({ to: e.target.value })} />
            <SelectField label="Regrouper" value={f.group_by} onChange={(e) => setF({ group_by: e.target.value })} options={GROUPS} />
            <SelectField label="Centre de formation" value={f.center_id} onChange={(e) => setF({ center_id: e.target.value })}
              options={[{ value: "", label: "Tous" }, ...(centers ?? []).map((c) => ({ value: String(c.id), label: c.name }))]} />
            <AsyncSelect label="Client" value={client} search={searchContacts} placeholder="Tous les clients" onChange={(o) => { setClient(o); setF({ client_id: o ? String(o.id) : "" }); }} />
            <SelectField label="Formateur" value={f.trainer_id} onChange={(e) => setF({ trainer_id: e.target.value })}
              options={[{ value: "", label: "Tous" }, ...(people ?? []).map((p) => ({ value: String(p.id), label: p.display_name }))]} />
            <SelectField label="Logiciel" value={f.software_id} onChange={(e) => setF({ software_id: e.target.value })}
              options={[{ value: "", label: "Tous" }, ...(software ?? []).map((s) => ({ value: String(s.id), label: s.name }))]} />
            <SelectField label="Modalité" value={f.modality} onChange={(e) => setF({ modality: e.target.value })}
              options={[{ value: "", label: "Toutes" }, { value: "PRESENTIEL", label: "Présentiel" }, { value: "DISTANCIEL", label: "Distanciel" }, { value: "HYBRIDE", label: "Hybride" }]} />
            <SelectField label="Format" value={f.group_format} onChange={(e) => setF({ group_format: e.target.value })}
              options={[{ value: "", label: "Tous" }, { value: "INDIVIDUEL", label: "Individuel" }, { value: "GROUPE", label: "Groupe" }]} />
            <SelectField label="OS" value={f.os} onChange={(e) => setF({ os: e.target.value })} options={[{ value: "", label: "Tous" }, { value: "PC", label: "PC" }, { value: "MAC", label: "Mac" }]} />
          </div>
        </Card>
        {error ? <Card><p className="muted">{(error as Error).message}</p></Card> : null}
        {isLoading || !data || !t ? <Loading /> : (
          <>
            <div className="stats">
              <Stat label="Heures travaillées" value={formatMinutes(t.minutes)} sub={`${formatMinutes(t.billable_minutes)} facturables`} />
              <Stat label="Formations" value={t.missions} sub={`${t.sessions} sessions réalisées`} />
              <Stat label="Stagiaires" value={t.participants} sub="distincts" />
              <Stat label="Facturé HT" value={formatMoney(t.invoiced_ht)} sub={`${t.invoices} facture(s)`} />
              <Stat label="Encaissé" value={formatMoney(t.paid)} tone={t.paid < t.invoiced_ht ? "warning" : "success"} />
            </div>
            <Card title={GROUPS.find((g) => g.value === data.group_by)?.label ?? "Résultats"}>
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>Regroupement</th><th style={{ textAlign: "right" }}>Heures travaillées</th><th style={{ textAlign: "right" }}>dont facturables</th>
                      <th style={{ textAlign: "right" }}>Formations</th><th style={{ textAlign: "right" }}>Stagiaires</th><th style={{ textAlign: "right" }}>Sessions</th>
                      <th style={{ textAlign: "right" }}>Facturé HT</th><th style={{ textAlign: "right" }}>dont temps</th><th style={{ textAlign: "right" }}>dont frais</th><th style={{ textAlign: "right" }}>Encaissé</th><th style={{ textAlign: "right" }}>Dépenses TTC</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.rows.map((r) => (
                      <tr key={r.key}>
                        <td>{r.label}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMinutes(r.minutes)}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMinutes(r.billable_minutes)}</td>
                        <td style={{ textAlign: "right" }}>{r.missions}</td>
                        <td style={{ textAlign: "right" }}>{r.participants}</td>
                        <td style={{ textAlign: "right" }}>{r.sessions}</td>
                        <td style={{ textAlign: "right" }} className="nowrap"><strong>{formatMoney(r.invoiced_ht)}</strong></td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(r.invoiced_time)}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(r.invoiced_expenses)}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(r.paid)}</td>
                        <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(r.expenses_ttc)}</td>
                      </tr>
                    ))}
                    {data.rows.length === 0 ? <tr><td colSpan={11} className="table-empty">Aucune donnée sur cette période avec ces critères.</td></tr> : null}
                  </tbody>
                  <tfoot>
                    <tr>
                      <td><strong>Total</strong></td>
                      <td style={{ textAlign: "right" }}><strong>{formatMinutes(t.minutes)}</strong></td><td style={{ textAlign: "right" }}>{formatMinutes(t.billable_minutes)}</td>
                      <td style={{ textAlign: "right" }}><strong>{t.missions}</strong></td><td style={{ textAlign: "right" }}><strong>{t.participants}</strong></td><td style={{ textAlign: "right" }}>{t.sessions}</td>
                      <td style={{ textAlign: "right" }}><strong>{formatMoney(t.invoiced_ht)}</strong></td><td style={{ textAlign: "right" }}>{formatMoney(t.invoiced_time)}</td>
                      <td style={{ textAlign: "right" }}>{formatMoney(t.invoiced_expenses)}</td><td style={{ textAlign: "right" }}><strong>{formatMoney(t.paid)}</strong></td><td style={{ textAlign: "right" }}><strong>{formatMoney(t.expenses_ttc)}</strong></td>
                    </tr>
                  </tfoot>
                </table>
              </div>
              <p className="muted small">
                Heures : feuilles de temps datées dans la période. Formations et stagiaires : missions dont la première session (ou la date de début) est dans la
                période, hors annulées ; un stagiaire présent sur plusieurs formations compte une fois. Facturé : factures validées ou payées par date de facture,
                avoirs déduits. Encaissé : paiements par date de paiement. Une facture ou un paiement sans mission (facture manuelle ou facture mensuelle de
                centre) est rattaché au centre ou au client, mais pas à un logiciel ni à un formateur. Dépenses : toutes pour les regroupements par mois, année ou total ;
                seulement celles rattachées à une mission pour les autres regroupements.
              </p>
            </Card>
            <Card title="Exporter le détail (CSV)">
              <div className="inline-form">
                <a className="btn" href={exportUrl("timesheets")} download>Feuilles de temps</a>
                <a className="btn" href={exportUrl("missions")} download>Formations</a>
                <a className="btn" href={exportUrl("invoices")} download>Factures</a>
                <a className="btn" href={exportUrl("payments")} download>Paiements</a>
                <a className="btn" href={exportUrl("expenses")} download>Dépenses</a>
                <Button variant="ghost" onClick={() => { setClient(null); setF({ center_id: "", client_id: "", trainer_id: "", software_id: "", modality: "", group_format: "", os: "" }); }}>Effacer les filtres</Button>
              </div>
              <p className="muted small">Chaque ligne du détail porte les critères (centre, client, formateur, logiciel, modalité, format, OS) : vous pouvez faire vos propres tableaux croisés dans Excel.</p>
            </Card>
          </>
        )}
      </div>
    </>
  );
}
