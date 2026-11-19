import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import type { Invoice } from "../../api/types";
import { Badge, Button, Card, Checkbox, ConfirmDialog, Loading, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { formatMinutes, formatMoney, todayIso } from "../../lib/format";
import { useUrlState } from "../../lib/hooks";

// Structures renvoyées par /billing/monthly (miroir du serveur).
interface MonthlyTask { task_name: string | null; hourly_rate: number; minutes: number; amount: number; count: number }
interface MonthlyMission { project_id: number; mission_id: number | null; reference: string | null; title: string; client_name: string; tasks: MonthlyTask[]; minutes: number; amount: number; expenses_total: number }
interface MonthlyGroup { partner_id: number; partner_name: string; is_center: boolean; missions: MonthlyMission[]; minutes: number; amount: number; expenses_total: number; total: number; minutes_before: number }
interface MonthlyReport { month: string; month_label: string; groups: MonthlyGroup[]; total: number }

// Facturation mensuelle : choix du mois, un bloc par centre (ou client direct), bouton "Créer la facture".
export function MonthlyBillingPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const [f, setF] = useUrlState({ mois: todayIso().slice(0, 7) });
  const [invoiceDate, setInvoiceDate] = useState(todayIso());
  const [withExpenses, setWithExpenses] = useState(true);
  const [confirm, setConfirm] = useState<MonthlyGroup | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["billing-monthly", f.mois], queryFn: () => api.get<MonthlyReport>(`/billing/monthly?month=${f.mois}`), enabled: /^\d{4}-\d{2}$/.test(f.mois) });
  // Crée la facture mensuelle du centre puis ouvre le brouillon.
  const create = useMutation({
    mutationFn: (g: MonthlyGroup) => api.post<Invoice>("/billing/monthly", { partner_id: g.partner_id, month: f.mois, invoice_date: invoiceDate, include_expenses: withExpenses }),
    onSuccess: (inv) => {
      qc.invalidateQueries({ queryKey: ["billing-monthly"] });
      qc.invalidateQueries({ queryKey: ["timesheets"] });
      qc.invalidateQueries({ queryKey: ["invoices"] });
      toast("Facture brouillon créée");
      nav(`/facturation/${inv.id}`);
    },
    onError: (e) => { setConfirm(null); toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"); },
  });

  return (
    <>
      <div className="page-header">
        <div className="page-header-left"><div className="breadcrumb"><Link to="/facturation">Factures</Link><span>Facturation mensuelle par centre</span></div></div>
      </div>
      <div className="page-body wide form-stack">
        <Card>
          <div className="grid-3">
            <TextField label="Mois" type="month" value={f.mois} onChange={(e) => setF({ mois: e.target.value })} />
            <TextField label="Date de facture" type="date" value={invoiceDate} onChange={(e) => setInvoiceDate(e.target.value)} />
            <div className="field"><label>Frais</label><Checkbox label="Ajouter les frais en attente des missions" checked={withExpenses} onChange={(e) => setWithExpenses(e.target.checked)} /></div>
          </div>
          <p className="muted small">
            Temps facturables non encore facturés jusqu'à la fin du mois choisi, regroupés par centre de formation (ou client direct quand la mission n'a pas de
            centre), puis par mission et par tâche. Une facture brouillon est créée par centre ; les lignes reprennent les heures par mission et par tâche au tarif
            de chaque mission. Les temps plus anciens non facturés sont inclus et signalés.
          </p>
        </Card>
        {isLoading || !data ? <Loading /> : data.groups.length === 0 ? <Card><p className="muted">Rien à facturer pour {data.month_label}.</p></Card> : (
          <>
            {data.groups.map((g) => (
              <Card key={g.partner_id} title={<>{g.partner_name} {g.is_center ? <Badge tone="info">Centre</Badge> : <Badge>Client direct</Badge>}</>}
                actions={<Button variant="primary" onClick={() => setConfirm(g)}>Créer la facture ({formatMoney(g.total)})</Button>}>
                {g.minutes_before > 0 ? <p className="muted small">Dont {formatMinutes(g.minutes_before)} datées avant {data.month_label} (non facturées jusqu'ici).</p> : null}
                <table className="table">
                  <thead><tr><th>Mission</th><th>Client</th><th>Tâche</th><th style={{ textAlign: "right" }}>Heures</th><th style={{ textAlign: "right" }}>Tarif</th><th style={{ textAlign: "right" }}>Montant</th></tr></thead>
                  <tbody>
                    {g.missions.map((m) => (
                      <>
                        {m.tasks.map((t, i) => (
                          <tr key={`${m.project_id}-${i}`}>
                            <td>{i === 0 ? (m.mission_id ? <Link to={`/missions/${m.mission_id}`}>{m.reference} {m.title}</Link> : <Link to={`/projets/${m.project_id}`}>{m.title}</Link>) : ""}</td>
                            <td>{i === 0 ? m.client_name : ""}</td>
                            <td>{t.task_name ?? <span className="muted">sans tâche</span>} <span className="muted small">({t.count})</span></td>
                            <td style={{ textAlign: "right" }} className="nowrap">{formatMinutes(t.minutes)}</td>
                            <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(t.hourly_rate)}/h</td>
                            <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(t.amount)}</td>
                          </tr>
                        ))}
                        {m.expenses_total > 0 ? (
                          <tr key={`${m.project_id}-frais`} className="muted">
                            <td /><td /><td>Frais en attente</td><td /><td /><td style={{ textAlign: "right" }}>{formatMoney(m.expenses_total)}</td>
                          </tr>
                        ) : null}
                      </>
                    ))}
                  </tbody>
                  <tfoot>
                    <tr><td colSpan={3}><strong>Total {g.partner_name}</strong></td><td style={{ textAlign: "right" }}><strong>{formatMinutes(g.minutes)}</strong></td><td />
                      <td style={{ textAlign: "right" }}><strong>{formatMoney(g.total)}</strong>{g.expenses_total > 0 ? <div className="muted small">dont frais {formatMoney(g.expenses_total)}</div> : null}</td></tr>
                  </tfoot>
                </table>
              </Card>
            ))}
            <Card><p style={{ textAlign: "right" }}><strong>Total à facturer pour {data.month_label} : {formatMoney(data.total)}</strong></p></Card>
          </>
        )}
      </div>
      <ConfirmDialog open={!!confirm} title="Créer la facture mensuelle" confirmLabel="Créer le brouillon" loading={create.isPending}
        onCancel={() => setConfirm(null)} onConfirm={() => confirm && create.mutate(confirm)}
        message={confirm ? `Une facture brouillon de ${formatMoney(withExpenses ? confirm.total : confirm.amount)} sera créée pour ${confirm.partner_name} (${formatMinutes(confirm.minutes)} sur ${confirm.missions.length} mission(s)). Les temps seront marqués facturés ; vous pourrez relire et modifier la facture avant validation.` : ""} />
    </>
  );
}
