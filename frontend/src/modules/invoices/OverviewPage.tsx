import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import type { Invoice } from "../../api/types";
import { Stat } from "../../components/Fields";
import { Badge, Button, Card, Loading, PageHeader, SelectField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { formatDate, formatMoney, todayIso } from "../../lib/format";
import { useUrlState } from "../../lib/hooks";

// Structures renvoyées par /invoices/overview (miroir du serveur).
interface Bucket { count: number; amount: number }
interface ClientDue { partner_id: number; partner_name: string; count: number; amount_due: number; overdue_due: number; oldest_due_date: string | null }
interface Pending { id: number; number: string | null; partner_id: number; partner_name: string; invoice_date: string; due_date: string; sent_at: string | null; last_reminder_at: string | null; total_ttc: number; amount_due: number; days_late: number }
interface Overview {
  year: number; today: string; drafts: Bucket; not_sent: Bucket; sent_waiting: Bucket; overdue: Bucket;
  overdue_1_30: Bucket; overdue_31_60: Bucket; overdue_61_90: Bucket; overdue_90_plus: Bucket;
  paid_year: Bucket; credit_notes_year: Bucket; issued_year: Bucket; collected_year: number; collected_month: number; avg_payment_days: number | null;
  by_client: ClientDue[]; pending: Pending[];
  einvoicing_enabled: boolean; einvoice_to_send: Bucket; einvoice_sent: Bucket; einvoice_accepted: Bucket; einvoice_refused: Bucket; einvoice_out_of_scope: Bucket; einvoice_untracked: Bucket;
}

const sub = (b: Bucket) => `${b.count} facture${b.count > 1 ? "s" : ""}`;

// État des lieux de la facturation : où en sont les factures (brouillon, à envoyer, envoyées, en retard, payées),
// ancienneté des retards, reste dû par client et liste des factures en attente avec actions rapides.
export function InvoiceOverviewPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const year = todayIso().slice(0, 4);
  const [f, setF] = useUrlState({ year });
  const { data, isLoading } = useQuery({ queryKey: ["invoices", "overview", f.year], queryFn: () => api.get<Overview>(`/invoices/overview?year=${f.year}`), placeholderData: (p) => p });
  const years = Array.from({ length: 6 }, (_, i) => String(Number(year) - i));
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");
  const refresh = () => { qc.invalidateQueries({ queryKey: ["invoices"] }); qc.invalidateQueries({ queryKey: ["dashboard"] }); };
  // Actions rapides depuis la liste des factures en attente.
  const mark = useMutation({
    mutationFn: ({ id, action }: { id: number; action: "sent" | "reminder" }) => api.post<Invoice>(`/invoices/${id}/${action}`, action === "sent" ? {} : undefined),
    onSuccess: (_, v) => { refresh(); toast(v.action === "sent" ? "Facture marquée envoyée" : "Relance enregistrée"); },
    onError,
  });

  if (isLoading || !data) return <Loading />;
  const totalPending = data.not_sent.amount + data.sent_waiting.amount + data.overdue.amount;
  const agingMax = Math.max(1, data.overdue_1_30.amount, data.overdue_31_60.amount, data.overdue_61_90.amount, data.overdue_90_plus.amount);
  const aging = [
    { label: "1 à 30 jours", b: data.overdue_1_30, tone: "warning" },
    { label: "31 à 60 jours", b: data.overdue_31_60, tone: "warning" },
    { label: "61 à 90 jours", b: data.overdue_61_90, tone: "danger" },
    { label: "Plus de 90 jours", b: data.overdue_90_plus, tone: "danger" },
  ];

  return (
    <>
      <PageHeader title="État des lieux de la facturation" actions={<SelectField label="" aria-label="Année" value={f.year} onChange={(e) => setF({ year: e.target.value })} options={years.map((y) => ({ value: y, label: y }))} />}>
        <div className="breadcrumb"><Link to="/facturation">Factures</Link><span>État des lieux au {formatDate(data.today)}</span></div>
      </PageHeader>
      <div className="page-body wide form-stack">
        <Card title={`Où en sont les factures (reste dû : ${formatMoney(totalPending)})`}>
          <div className="stats">
            <Stat label="Brouillons" value={formatMoney(data.drafts.amount)} sub={sub(data.drafts)} onClick={() => nav("/facturation?status=BROUILLON")} />
            <Stat label="Validées, à envoyer" value={formatMoney(data.not_sent.amount)} sub={sub(data.not_sent)} tone={data.not_sent.count ? "warning" : undefined} onClick={() => nav("/facturation?status=VALIDEE&sent=false")} />
            <Stat label="Envoyées, dans les délais" value={formatMoney(data.sent_waiting.amount)} sub={sub(data.sent_waiting)} onClick={() => nav("/facturation?status=VALIDEE")} />
            <Stat label="En retard de règlement" value={formatMoney(data.overdue.amount)} sub={sub(data.overdue)} tone={data.overdue.count ? "danger" : "success"} onClick={() => nav("/facturation?overdue=true")} />
          </div>
          <p className="muted small">Brouillons : non numérotés, modifiables. À envoyer : validées mais pas encore marquées envoyées (bouton "Marquer envoyée" sur la facture). En retard : échéance dépassée, quel que soit l'état d'envoi.</p>
        </Card>
        <Card title={`Bilan ${data.year}`}>
          <div className="stats">
            <Stat label="Facturé HT" value={formatMoney(data.issued_year.amount)} sub={`${data.issued_year.count} document(s), avoirs déduits`} />
            <Stat label="Factures payées" value={formatMoney(data.paid_year.amount)} sub={`${sub(data.paid_year)} (TTC)`} tone="success" onClick={() => nav("/facturation?status=PAYEE")} />
            <Stat label="Encaissé" value={formatMoney(data.collected_year)} sub={`dont ${formatMoney(data.collected_month)} ce mois`} />
            <Stat label="Avoirs" value={formatMoney(data.credit_notes_year.amount)} sub={sub(data.credit_notes_year)} onClick={() => nav("/facturation?kind=AVOIR")} />
            <Stat label="Délai moyen de paiement" value={data.avg_payment_days === null ? "-" : `${data.avg_payment_days} j`} sub="date de facture -> dernier paiement" />
          </div>
        </Card>
        {data.einvoicing_enabled ? (
          <Card title={`Facturation électronique ${data.year}`}>
            <div className="stats">
              <Stat label="À transmettre" value={data.einvoice_to_send.count} sub={formatMoney(data.einvoice_to_send.amount)} tone={data.einvoice_to_send.count ? "warning" : "success"} onClick={() => nav("/facturation?einvoice=A_TRANSMETTRE")} />
              <Stat label="Transmises" value={data.einvoice_sent.count} sub={formatMoney(data.einvoice_sent.amount)} onClick={() => nav("/facturation?einvoice=TRANSMISE")} />
              <Stat label="Acceptées" value={data.einvoice_accepted.count} sub={formatMoney(data.einvoice_accepted.amount)} tone="success" onClick={() => nav("/facturation?einvoice=ACCEPTEE")} />
              <Stat label="Refusées" value={data.einvoice_refused.count} sub={formatMoney(data.einvoice_refused.amount)} tone={data.einvoice_refused.count ? "danger" : undefined} onClick={() => nav("/facturation?einvoice=REFUSEE")} />
              <Stat label="Hors champ (particuliers)" value={data.einvoice_out_of_scope.count} sub={formatMoney(data.einvoice_out_of_scope.amount)} />
              {data.einvoice_untracked.count ? <Stat label="Non suivies" value={data.einvoice_untracked.count} sub="émises avant l'activation" /> : null}
            </div>
            <p className="muted small">Déposez les fichiers (PDF Factur-X ou XML) sur votre plateforme agréée depuis chaque facture, puis renseignez le statut renvoyé par la plateforme.</p>
          </Card>
        ) : null}
        <div className="grid-2 cards">
          <Card title="Retards par ancienneté">
            {data.overdue.count === 0 ? <p className="muted">Aucune facture en retard.</p> : (
              <table className="table compact">
                <tbody>
                  {aging.map((a) => (
                    <tr key={a.label}>
                      <td>{a.label} <span className="muted small">({a.b.count})</span></td>
                      <td style={{ textAlign: "right" }} className="nowrap"><strong>{formatMoney(a.b.amount)}</strong></td>
                      <td style={{ width: "40%" }}><div className="bar-track"><div className={`bar-fill ${a.tone}`} style={{ width: `${Math.round((a.b.amount * 100) / agingMax)}%` }} /></div></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Card>
          <Card title="Reste dû par client">
            {data.by_client.length === 0 ? <p className="muted">Aucune facture en attente.</p> : (
              <table className="table compact">
                <thead><tr><th>Client</th><th style={{ textAlign: "right" }}>Factures</th><th style={{ textAlign: "right" }}>Reste dû</th><th style={{ textAlign: "right" }}>dont en retard</th><th>Plus ancienne échéance</th></tr></thead>
                <tbody>
                  {data.by_client.map((c) => (
                    <tr key={c.partner_id}>
                      <td><Link to={`/facturation?partner_id=${c.partner_id}&status=VALIDEE`}>{c.partner_name}</Link></td>
                      <td style={{ textAlign: "right" }}>{c.count}</td>
                      <td style={{ textAlign: "right" }} className="nowrap"><strong>{formatMoney(c.amount_due)}</strong></td>
                      <td style={{ textAlign: "right" }} className={`nowrap ${c.overdue_due ? "text-danger" : ""}`}>{c.overdue_due ? formatMoney(c.overdue_due) : "-"}</td>
                      <td className="nowrap">{formatDate(c.oldest_due_date)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Card>
        </div>
        <Card title={`Factures en attente de règlement (${data.pending.length})`}>
          {data.pending.length === 0 ? <p className="muted">Toutes les factures validées sont réglées.</p> : (
            <div className="table-wrap">
              <table className="table">
                <thead><tr><th>Numéro</th><th>Client</th><th>Date</th><th>Échéance</th><th>Envoi</th><th>Dernière relance</th><th style={{ textAlign: "right" }}>Total TTC</th><th style={{ textAlign: "right" }}>Reste dû</th><th>Situation</th><th /></tr></thead>
                <tbody>
                  {data.pending.map((p) => (
                    <tr key={p.id}>
                      <td><Link to={`/facturation/${p.id}`}>{p.number}</Link></td>
                      <td><Link to={`/contacts/${p.partner_id}`}>{p.partner_name}</Link></td>
                      <td className="nowrap">{formatDate(p.invoice_date)}</td>
                      <td className="nowrap">{formatDate(p.due_date)}</td>
                      <td className="nowrap">{p.sent_at ? formatDate(p.sent_at) : <Badge tone="warning">Non envoyée</Badge>}</td>
                      <td className="nowrap">{p.last_reminder_at ? formatDate(p.last_reminder_at) : <span className="muted">-</span>}</td>
                      <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(p.total_ttc)}</td>
                      <td style={{ textAlign: "right" }} className="nowrap"><strong>{formatMoney(p.amount_due)}</strong></td>
                      <td>{p.days_late > 0 ? <Badge tone="danger">En retard de {p.days_late} j</Badge> : p.sent_at ? <Badge tone="info">Dans les délais</Badge> : <Badge tone="warning">À envoyer</Badge>}</td>
                      <td className="nowrap">
                        {!p.sent_at ? <Button variant="ghost" onClick={() => mark.mutate({ id: p.id, action: "sent" })}>Envoyée</Button> : null}
                        {p.days_late > 0 ? <Button variant="ghost" onClick={() => mark.mutate({ id: p.id, action: "reminder" })}>Relance faite</Button> : null}
                        <a className="btn btn-ghost" href={`/api/invoices/${p.id}/pdf`} target="_blank" rel="noreferrer">PDF</a>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
