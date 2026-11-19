import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router-dom";
import { api } from "../../api/client";
import type { CompanySettings } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Stat } from "../../components/Fields";
import { Badge, Card, Loading, PageHeader } from "../../components/ui";
import { formatMinutes, formatMoney } from "../../lib/format";

interface Dashboard {
  contacts: number;
  clients: number;
  month_minutes: number;
  month_billable_minutes: number;
  uninvoiced_amount: number;
  pending_invoices: number;
  pending_amount: number;
  overdue_invoices: number;
  overdue_amount: number;
  paid_invoices_year: number;
  revenue_year: number;
  revenue_month: number;
  expenses_year: number;
  expenses_month: number;
  expenses_to_pay: number;
  draft_invoices: number;
  missions_to_invoice: number;
  sessions_this_week: number;
  revenue_by_month: [string, number][];
  can_see_finance: boolean;
}

// Mois abrégés pour l'histogramme.
const MONTHS = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

// Histogramme CSS du chiffre d'affaires HT des 12 derniers mois (hauteur relative au maximum).
function RevenueChart({ data }: { data: [string, number][] }) {
  const max = Math.max(1, ...data.map((d) => d[1]));
  return (
    <div className="bar-chart" role="img" aria-label="Chiffre d'affaires HT des 12 derniers mois">
      {data.map(([ym, v]) => (
        <div key={ym} className="bar-col" title={`${MONTHS[Number(ym.slice(5)) - 1]} ${ym.slice(0, 4)} : ${formatMoney(v)}`}>
          <span className="bar-value">{v ? `${Math.round(v / 100).toLocaleString("fr-FR")} €` : ""}</span>
          <div className="bar" style={{ height: `${Math.max(0, (v / max) * 100)}%` }} />
          <span className="bar-label">{MONTHS[Number(ym.slice(5)) - 1]}</span>
        </div>
      ))}
    </div>
  );
}

// Tableau de bord : indicateurs généraux pour tous, bloc financier pour ADMIN/GESTIONNAIRE.
export function DashboardPage() {
  const nav = useNavigate();
  const { user } = useAuth();
  const { data } = useQuery({ queryKey: ["dashboard"], queryFn: () => api.get<Dashboard>("/dashboard") });
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: () => api.get<CompanySettings>("/settings/company") });
  if (!data) return <Loading />;
  const year = new Date().getFullYear();

  return (
    <>
      <PageHeader title="Tableau de bord" />
      <div className="page-body wide">
        // Liste de mise en route tant que l'entreprise n'est pas renseignée.
        {hasRole(user, "ADMIN") && settings && !settings.legal_name ? (
          <Card title="Mise en route">
            <ul className="checklist">
              <li><Badge tone="warning">À faire</Badge><Link to="/parametres/entreprise">Renseigner l'entreprise (identité, NDA, banque, mentions légales)</Link></li>
              <li><Badge tone="info">Conseillé</Badge><Link to="/contacts/import">Importer les contacts depuis Odoo</Link></li>
            </ul>
          </Card>
        ) : null}
        // Indicateurs cliquables : chacun ouvre la liste filtrée correspondante.
        <div className="stats">
          <Stat label="Contacts" value={data.contacts} onClick={() => nav("/contacts")} />
          <Stat label="Clients" value={data.clients} onClick={() => nav("/contacts?role=client")} />
          <Stat label={data.can_see_finance ? "Heures du mois" : "Mes heures du mois"} value={formatMinutes(data.month_minutes)} onClick={() => nav("/temps?vue=liste&periode=mois")} />
          <Stat label="Heures facturables" value={formatMinutes(data.month_billable_minutes)} />
          <Stat label="Sessions cette semaine" value={data.sessions_this_week} onClick={() => nav("/missions?bucket=semaine")} />
        </div>
        {data.can_see_finance ? (
          <>
            <div className="stats">
              <Stat label="Montant facturable en attente" value={formatMoney(data.uninvoiced_amount)} tone={data.uninvoiced_amount ? "warning" : undefined} onClick={() => nav("/facturation/a-facturer")} />
              <Stat label="Factures en attente de paiement" value={formatMoney(data.pending_amount)} sub={`${data.pending_invoices} facture(s)`} onClick={() => nav("/facturation?status=VALIDEE")} />
              <Stat label="Dont en retard" value={formatMoney(data.overdue_amount)} sub={`${data.overdue_invoices} facture(s)`} tone={data.overdue_invoices ? "danger" : undefined} onClick={() => nav("/facturation?overdue=true")} />
              <Stat label={`Factures payées ${year}`} value={data.paid_invoices_year} onClick={() => nav("/facturation?status=PAYEE")} />
              <Stat label="Brouillons" value={data.draft_invoices} onClick={() => nav("/facturation?status=BROUILLON")} />
              <Stat label="Missions à facturer" value={data.missions_to_invoice} onClick={() => nav("/missions?bucket=a_facturer")} />
            </div>
            <div className="stats">
              <Stat label={`Dépenses ${year} (TTC)`} value={formatMoney(data.expenses_year)} sub={`${formatMoney(data.expenses_month)} ce mois`} onClick={() => nav("/depenses")} />
              <Stat label="Dépenses à payer" value={formatMoney(data.expenses_to_pay)} tone={data.expenses_to_pay ? "warning" : undefined} onClick={() => nav("/depenses?status=A_PAYER")} />
              <Stat label={`Résultat brut ${year}`} value={formatMoney(data.revenue_year - data.expenses_year)} sub="CA HT - dépenses TTC (indicatif)" tone={data.revenue_year - data.expenses_year < 0 ? "danger" : undefined} />
            </div>
            <Card title={`Chiffre d'affaires HT : ${formatMoney(data.revenue_year)} en ${year} (dont ${formatMoney(data.revenue_month)} ce mois)`}>
              <RevenueChart data={data.revenue_by_month} />
              <p className="muted small">Factures validées moins avoirs, par date de facture.</p>
            </Card>
          </>
        ) : null}
      </div>
    </>
  );
}
