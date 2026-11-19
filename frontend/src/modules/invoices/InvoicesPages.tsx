import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import { EINVOICE_STATUS, EXPENSE_LABELS, INVOICE_STATUS, OPERATION_CATEGORY_LABELS, type Billable, type CompanySettings, type EinvoiceStatus, type Invoice, type InvoiceLine, type Product } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { MoneyField, StatusBadge } from "../../components/Fields";
import { AsyncSelect, Option } from "../../components/Pickers";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Card, Checkbox, ConfirmDialog, Loading, Modal, PageHeader, Pagination, SearchInput, SelectField, TextAreaField, TextField } from "../../components/ui";
import { bpToPercent, formatDate, formatDateTime, formatMinutes, formatMoney, formatQty, parseQty, percentToBp, relativeDue, todayIso } from "../../lib/format";
import { useDebounced, useUrlState } from "../../lib/hooks";
import { searchContacts } from "../../lib/search";

// Réponse de /invoices : page + sommes.
interface ListResponse {
  items: Invoice[];
  total: number;
  page: number;
  per_page: number;
  sum_ht: number;
  sum_ttc: number;
  sum_due: number;
}

// Liste des factures : filtres (URL), totaux, accès à la facturation mensuelle et aux éléments à facturer.
export function InvoicesListPage() {
  const nav = useNavigate();
  const [f, setF] = useUrlState({ q: "", status: "", kind: "", overdue: "", sent: "", einvoice: "", partner_id: "", page: "1" });
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: () => api.get<CompanySettings>("/settings/company") });
  const [text, setText] = useState(f.q);
  const q = useDebounced(text, 250);
  // Initialisation : depuis la facture chargée, ou valeurs par défaut des paramètres pour une nouvelle.
  useEffect(() => {
    if (q !== f.q) setF({ q, page: "1" });
  }, [q]); // eslint-disable-line react-hooks/exhaustive-deps
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v) params.set(k, v);
  const { data, isLoading } = useQuery({ queryKey: ["invoices", params.toString()], queryFn: () => api.get<ListResponse>(`/invoices?${params}`), placeholderData: (p) => p });

  const columns: Column<Invoice>[] = [
    { key: "n", header: "Numéro", render: (i) => <strong>{i.number ?? <span className="muted">Brouillon</span>}</strong> },
    { key: "c", header: "Client", render: (i) => i.partner_name },
    { key: "p", header: "Objet", render: (i) => <span className="small">{i.project_name ?? ""}</span> },
    { key: "d", header: "Date", render: (i) => formatDate(i.invoice_date) },
    {
      key: "e", header: "Échéance",
      render: (i) => {
        if (i.kind !== "FACTURE" || i.status !== "VALIDEE") return "";
        const r = relativeDue(i.due_date);
        return <span className={r.late ? "text-danger" : ""} title={formatDate(i.due_date)}>{r.text}</span>;
      },
    },
    { key: "sent", header: "Envoyée", render: (i) => (i.status === "BROUILLON" || i.kind === "AVOIR" ? "" : i.sent_at ? <span title={formatDate(i.sent_at)}>{formatDate(i.sent_at)}</span> : <span className="muted">non</span>) },
    { key: "r", header: "Dernier rappel", render: (i) => formatDate(i.last_reminder_at) },
    { key: "ht", header: "Hors taxes", align: "right", render: (i) => <span className="nowrap">{i.kind === "AVOIR" ? "-" : ""}{formatMoney(i.total_ht)}</span> },
    { key: "ttc", header: "Total", align: "right", render: (i) => <strong className="nowrap">{i.kind === "AVOIR" ? "-" : ""}{formatMoney(i.total_ttc)}</strong> },
    { key: "due", header: "Montant dû", align: "right", render: (i) => <span className="nowrap">{formatMoney(i.amount_due)}</span> },
    { key: "s", header: "Statut", render: (i) => <>{i.kind === "AVOIR" ? <Badge tone="warning">Avoir</Badge> : null} <StatusBadge map={INVOICE_STATUS} value={i.status} />{i.einvoice_status && i.einvoice_status !== "HORS_CHAMP" ? <div><Badge tone={EINVOICE_STATUS[i.einvoice_status].tone}>e-facture : {EINVOICE_STATUS[i.einvoice_status].label}</Badge></div> : null}</> },
  ];

  return (
    <>
      <PageHeader title="Factures clients" actions={<Button variant="primary" onClick={() => nav("/facturation/nouvelle")}>Nouveau</Button>}>
        <SearchInput value={text} onChange={setText} placeholder="Numéro, client, objet..." />
        {data ? <Pagination page={data.page} perPage={data.per_page} total={data.total} onPage={(p) => setF({ page: String(p) })} /> : null}
      </PageHeader>
      <div className="filter-bar">
        <select aria-label="Statut" value={f.status} onChange={(e) => setF({ status: e.target.value, page: "1" })}>
          <option value="">Tous les statuts</option>
          {Object.entries(INVOICE_STATUS).map(([k, s]) => <option key={k} value={k}>{s.label}</option>)}
        </select>
        <select aria-label="Type" value={f.kind} onChange={(e) => setF({ kind: e.target.value, page: "1" })}>
          <option value="">Factures et avoirs</option>
          <option value="FACTURE">Factures</option>
          <option value="AVOIR">Avoirs</option>
        </select>
        <select aria-label="Envoi" value={f.sent} onChange={(e) => setF({ sent: e.target.value, page: "1" })}>
          <option value="">Envoyées ou non</option>
          <option value="false">Non envoyées</option>
          <option value="true">Envoyées</option>
        </select>
        {settings?.einvoicing_enabled || f.einvoice ? (
          <select aria-label="Facture électronique" value={f.einvoice} onChange={(e) => setF({ einvoice: e.target.value, page: "1" })}>
            <option value="">e-facture : tous</option>
            {Object.entries(EINVOICE_STATUS).map(([k, v]) => <option key={k} value={k}>{v.label}</option>)}
          </select>
        ) : null}
        {f.partner_id ? <Button variant="ghost" onClick={() => setF({ partner_id: "", page: "1" })}>Retirer le filtre client ×</Button> : null}
        <Checkbox label="En retard uniquement" checked={f.overdue === "true"} onChange={(e) => setF({ overdue: e.target.checked ? "true" : "", page: "1" })} />
        <span className="filter-actions">
          <Link className="btn btn-primary" to="/facturation/etat">État des lieux</Link>
          <Link className="btn btn-ghost" to="/facturation/mensuelle">Facturation mensuelle par centre</Link>
          <Link className="btn btn-ghost" to="/facturation/a-facturer">Temps et frais à facturer</Link>
        </span>
      </div>
      <div className="page-body wide">
        {isLoading || !data ? <Loading /> : (
          <>
            <DataTable columns={columns} rows={data.items} rowKey={(i) => i.id} onRowClick={(i) => nav(`/facturation/${i.id}`)} empty="Aucune facture" />
            <div className="totals-row">
              <span>Total HT : <strong>{formatMoney(data.sum_ht)}</strong></span>
              <span>Total TTC : <strong>{formatMoney(data.sum_ttc)}</strong></span>
              <span>Reste dû : <strong>{formatMoney(data.sum_due)}</strong></span>
            </div>
          </>
        )}
      </div>
    </>
  );
}

// Ligne en cours d'édition : quantité et TVA en texte libre jusqu'à l'enregistrement.
interface EditLine {
  id?: number;
  product_id?: number | null;
  description: string;
  qty: string;
  unit: string;
  unit_price: number;
  vat: string;
  source_type?: string;
  linked_items?: number;
}

// Ligne serveur -> ligne éditable.
function toEditLine(l: InvoiceLine): EditLine {
  return { id: l.id, product_id: l.product_id, description: l.description, qty: formatQty(l.quantity_milli), unit: l.unit, unit_price: l.unit_price, vat: bpToPercent(l.vat_rate_bp), source_type: l.source_type, linked_items: l.linked_items };
}

// Fiche facture : brouillon éditable (en-tête + lignes), puis validation, paiements, avoir, PDF.
export function InvoicePage() {
  const { id } = useParams();
  const isNew = id === "nouvelle";
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({ queryKey: ["invoice", id], queryFn: () => api.get<Invoice>(`/invoices/${id}`), enabled: !isNew });
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: () => api.get<CompanySettings>("/settings/company") });
  const { data: products } = useQuery({ queryKey: ["products"], queryFn: () => api.get<Product[]>("/products") });
  // TVA : toujours visible si l'entreprise est assujettie ; sinon activable à la demande sur cette facture.
  const [withVat, setWithVat] = useState(false);
  const vatEnabled = settings?.vat_regime === "ASSUJETTI" || withVat;
  const defaultVat = settings ? bpToPercent(settings.default_vat_rate_bp) : "0";

  const [partner, setPartner] = useState<Option | null>(null);
  const [head, setHead] = useState({ invoice_date: todayIso(), due_date: "", payment_terms: "", notes: "", operation_category: "SERVICES" as Invoice["operation_category"] });
  const [lines, setLines] = useState<EditLine[]>([]);
  const [error, setError] = useState("");
  const [pay, setPay] = useState<{ amount: number; payment_date: string; method: string; reference: string } | null>(null);
  const [cancel, setCancel] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<"validate" | "delete" | null>(null);

  useEffect(() => {
    if (data) {
      setPartner({ id: data.partner_id, label: data.partner_name });
      setHead({ invoice_date: data.invoice_date, due_date: data.due_date, payment_terms: data.payment_terms, notes: data.notes, operation_category: data.operation_category });
      setLines((data.lines ?? []).map(toEditLine));
      if ((data.lines ?? []).some((l) => l.vat_rate_bp > 0)) setWithVat(true);
    } else if (isNew && settings) {
      setHead((h) => ({ ...h, payment_terms: settings.payment_terms, operation_category: settings.default_operation_category }));
      setLines([{ product_id: null, description: "", qty: "1", unit: "", unit_price: 0, vat: defaultVat }]);
    }
  }, [data, isNew, settings, defaultVat]);

  const onError = (e: unknown) => {
    setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    window.scrollTo({ top: 0, behavior: "smooth" });
  };
  // Après une action : cache mis à jour, listes liées invalidées, notification.
  const apply = (inv: Invoice, msg: string) => {
    qc.setQueryData(["invoice", String(inv.id)], inv);
    qc.invalidateQueries({ queryKey: ["invoices"] });
    qc.invalidateQueries({ queryKey: ["mission"] });
    qc.invalidateQueries({ queryKey: ["timesheets"] });
    setError("");
    toast(msg);
  };

  // Formulaire -> corps JSON ; quantités et taux convertis, erreurs de saisie signalées avant l'envoi.
  const buildBody = () => {
    const out = [];
    for (const l of lines) {
      const q = parseQty(l.qty);
      const v = percentToBp(l.vat || "0");
      if (q === null || q <= 0) throw new ApiError(400, `Quantité invalide : « ${l.qty} »`);
      if (v === null) throw new ApiError(400, `Taux de TVA invalide : « ${l.vat} »`);
      out.push({ id: l.id, product_id: l.product_id ?? null, description: l.description, quantity_milli: q, unit: l.unit, unit_price: l.unit_price, vat_rate_bp: v });
    }
    return { partner_id: partner?.id ?? 0, project_id: data?.project_id ?? null, invoice_date: head.invoice_date, due_date: head.due_date || null, payment_terms: head.payment_terms, notes: head.notes, operation_category: head.operation_category, lines: out };
  };
  // Enregistrement du brouillon.
  const save = useMutation({
    mutationFn: async () => (isNew ? api.post<Invoice>("/invoices", buildBody()) : api.put<Invoice>(`/invoices/${id}`, buildBody())),
    onSuccess: (inv) => { apply(inv, "Facture enregistrée"); if (isNew) nav(`/facturation/${inv.id}`, { replace: true }); },
    onError,
  });
  // Validation / relance.
  const action = useMutation({
    mutationFn: async (a: "validate" | "reminder" | "sent") => api.post<Invoice>(`/invoices/${id}/${a}`, a === "sent" ? {} : undefined),
    onSuccess: (inv, a) => { setConfirm(null); apply(inv, a === "validate" ? `Facture validée : ${inv.number}` : a === "sent" ? "Facture marquée envoyée" : "Relance enregistrée"); },
    onError: (e) => { setConfirm(null); onError(e); },
  });
  // Suppression du brouillon, paiements, avoir.
  const remove = useMutation({ mutationFn: () => api.del(`/invoices/${id}`), onSuccess: () => { qc.invalidateQueries({ queryKey: ["invoices"] }); toast("Brouillon supprimé"); nav("/facturation"); }, onError: (e) => { setConfirm(null); onError(e); } });
  const addPayment = useMutation({ mutationFn: () => api.post<Invoice>(`/invoices/${id}/payments`, pay), onSuccess: (inv) => { setPay(null); apply(inv, "Paiement enregistré"); }, onError: (e) => { setPay(null); onError(e); } });
  const delPayment = useMutation({ mutationFn: (pid: number) => api.del<Invoice>(`/payments/${pid}`), onSuccess: (inv) => apply(inv, "Paiement supprimé"), onError });
  const creditNote = useMutation({ mutationFn: () => api.post<Invoice>(`/invoices/${id}/cancel`, { reason: cancel ?? "" }), onSuccess: (av) => { setCancel(null); apply(av, `Avoir ${av.number} créé`); nav(`/facturation/${av.id}`); }, onError: (e) => { setCancel(null); onError(e); } });

  if ((!isNew && (isLoading || !data)) || !settings) return <Loading />;
  const draft = isNew || data?.status === "BROUILLON";
  const setLine = (i: number, patch: Partial<EditLine>) => setLines(lines.map((l, k) => (k === i ? { ...l, ...patch } : l)));
  // Choix d'un produit : remplit libellé, unité, prix et TVA de la ligne (la description reste modifiable ensuite).
  const applyProduct = (i: number, pid: string) => {
    const pr = (products ?? []).find((x) => String(x.id) === pid);
    if (!pr) return setLine(i, { product_id: null });
    const vat = pr.vat_rate_bp > 0 ? bpToPercent(pr.vat_rate_bp) : vatEnabled ? defaultVat : "0";
    if (pr.vat_rate_bp > 0) setWithVat(true);
    setLine(i, { product_id: pr.id, description: pr.description || pr.name, unit: pr.unit, unit_price: pr.unit_price, vat });
  };
  const productOptions = [{ value: "", label: "- produit -" }, ...(products ?? []).filter((x) => x.active || lines.some((l) => l.product_id === x.id)).map((x) => ({ value: String(x.id), label: `${x.name} (${formatMoney(x.unit_price)}${x.unit ? "/" + x.unit : ""})` }))];
  const preview = lines.map((l) => {
    const q = parseQty(l.qty) ?? 0;
    const ht = Math.round((q * l.unit_price) / 1000);
    const vat = Math.round((ht * (percentToBp(l.vat || "0") ?? 0)) / 10000);
    return { ht, vat };
  });
  const totals = { ht: preview.reduce((s, p) => s + p.ht, 0), vat: preview.reduce((s, p) => s + p.vat, 0) };
  const title = isNew ? "Nouvelle facture" : data!.number ? `${data!.kind === "AVOIR" ? "Avoir" : "Facture"} ${data!.number}` : "Facture brouillon";

  return (
    <>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb"><Link to="/facturation">Factures</Link><span>{title}</span></div>
          {data ? <StatusBadge map={INVOICE_STATUS} value={data.status} /> : null}
          {draft ? <Button variant="primary" loading={save.isPending} onClick={() => save.mutate()}>Enregistrer</Button> : null}
        </div>
        <div className="page-header-right">
          {data && draft ? <Button variant="primary" onClick={() => setConfirm("validate")}>Valider</Button> : null}
          {data ? <a className="btn btn-secondary" href={`/api/invoices/${data.id}/pdf`} target="_blank" rel="noreferrer">PDF</a> : null}
          {data && data.kind === "FACTURE" && data.status === "VALIDEE" ? (
            <>
              <Button onClick={() => setPay({ amount: data.amount_due, payment_date: todayIso(), method: "VIREMENT", reference: "" })}>Enregistrer un paiement</Button>
              {!data.sent_at ? <Button onClick={() => action.mutate("sent")}>Marquer envoyée</Button> : null}
              <Button variant="ghost" onClick={() => action.mutate("reminder")}>Relance faite</Button>
              <Button variant="ghost" className="text-danger" onClick={() => setCancel("")}>Annuler par un avoir</Button>
            </>
          ) : null}
          {data && draft ? <Button variant="ghost" className="text-danger" onClick={() => setConfirm("delete")}>Supprimer</Button> : null}
        </div>
      </div>
      <div className="page-body wide">
        <Alert>{error}</Alert>
        {data?.credit_note_id ? <Alert tone="info">Cette facture a été annulée par l'avoir <Link to={`/facturation/${data.credit_note_id}`}>n° {data.credit_note_id}</Link>.</Alert> : null}
        {data?.reversed_invoice_id ? <Alert tone="info">Avoir sur la facture <Link to={`/facturation/${data.reversed_invoice_id}`}>{data.reversed_number}</Link>.</Alert> : null}
        {!draft ? <Alert tone="info">Facture validée : ses données comptables ne sont plus modifiables. Pour la corriger, annulez-la par un avoir puis refacturez.</Alert> : null}
        <Card>
          <div className="grid-3">
            <AsyncSelect label="Client" value={partner} search={searchContacts} disabled={!draft || !!data?.project_id} onChange={setPartner} />
            <TextField label="Date de facture" type="date" disabled={!draft} value={head.invoice_date} onChange={(e) => setHead({ ...head, invoice_date: e.target.value })} />
            <TextField label="Échéance" type="date" disabled={!draft} value={head.due_date} hint={draft && !head.due_date ? `Par défaut : +${settings.default_due_days} jours` : undefined} onChange={(e) => setHead({ ...head, due_date: e.target.value })} />
            {settings.einvoicing_enabled ? <SelectField label="Catégorie d'opération" disabled={!draft} value={head.operation_category} onChange={(e) => setHead({ ...head, operation_category: e.target.value as Invoice["operation_category"] })}
              options={Object.entries(OPERATION_CATEGORY_LABELS).map(([value, label]) => ({ value, label }))} hint="Mention de la facture électronique" /> : null}
            {data?.project_name ? <div className="field"><label>Objet</label><div className="readonly">{data.mission_id ? <Link to={`/missions/${data.mission_id}`}>{data.project_name}</Link> : <Link to={`/projets/${data.project_id}`}>{data.project_name}</Link>}</div></div> : null}
            {!draft && data ? <div className="field"><label>Adresse de facturation (figée)</label><div className="readonly pre">{data.billing_name}{"\n"}{data.billing_address}</div></div> : null}
          </div>
        </Card>
        <Card title="Lignes">
          <div className="table-wrap">
            <table className="table lines-table">
              <thead>
                <tr>
                  <th>Description</th><th style={{ width: 90 }}>Quantité</th><th style={{ width: 80 }}>Unité</th><th style={{ width: 130 }}>Prix unitaire HT</th>
                  {vatEnabled || lines.some((l) => l.vat !== "0") ? <th style={{ width: 80 }}>TVA %</th> : null}
                  <th style={{ width: 120, textAlign: "right" }}>Total HT</th>{draft ? <th style={{ width: 40 }} /> : null}
                </tr>
              </thead>
              <tbody>
                {lines.map((l, i) => (
                  <tr key={i}>
                    <td>
                      {draft ? (
                        <>
                          <select aria-label="Produit" className="line-product" value={l.product_id ? String(l.product_id) : ""} onChange={(e) => applyProduct(i, e.target.value)}>
                            {productOptions.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
                          </select>
                          <textarea rows={2} aria-label="Description" value={l.description} onChange={(e) => setLine(i, { description: e.target.value })} />
                        </>
                      ) : <span className="pre">{l.description}</span>}
                      {l.linked_items ? <div className="muted small">{l.source_type === "FRAIS" ? "Frais lié" : `${l.linked_items} temps lié(s)`} : supprimer la ligne les rend de nouveau facturables</div> : null}
                    </td>
                    <td>{draft ? <input aria-label="Quantité" value={l.qty} onChange={(e) => setLine(i, { qty: e.target.value })} /> : l.qty}</td>
                    <td>{draft ? <input aria-label="Unité" value={l.unit} onChange={(e) => setLine(i, { unit: e.target.value })} /> : l.unit}</td>
                    <td>{draft ? <MoneyField label="" value={l.unit_price} onChange={(v) => setLine(i, { unit_price: v })} /> : formatMoney(l.unit_price)}</td>
                    {vatEnabled || lines.some((x) => x.vat !== "0") ? <td>{draft && vatEnabled ? <input aria-label="TVA" value={l.vat} onChange={(e) => setLine(i, { vat: e.target.value })} /> : `${l.vat} %`}</td> : null}
                    <td style={{ textAlign: "right" }} className="nowrap">{formatMoney(preview[i]?.ht ?? 0)}</td>
                    {draft ? <td><button className="icon-btn" aria-label="Supprimer la ligne" onClick={() => setLines(lines.filter((_, k) => k !== i))}>×</button></td> : null}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {draft ? (
            <div className="inline-form">
              <Button onClick={() => setLines([...lines, { product_id: null, description: "", qty: "1", unit: "", unit_price: 0, vat: vatEnabled ? defaultVat : "0" }])}>+ Ajouter une ligne</Button>
              {settings.vat_regime !== "ASSUJETTI" ? <Checkbox label="Appliquer la TVA sur cette facture" checked={withVat} onChange={(e) => { setWithVat(e.target.checked); setLines(lines.map((l) => ({ ...l, vat: e.target.checked ? (l.vat === "0" ? "20" : l.vat) : "0" }))); }} /> : null}
            </div>
          ) : null}
          <div className="invoice-totals">
            <div><span>Total HT</span><strong>{formatMoney(data && !draft ? data.total_ht : totals.ht)}</strong></div>
            {vatEnabled || totals.vat ? <div><span>TVA</span><strong>{formatMoney(data && !draft ? data.total_vat : totals.vat)}</strong></div> : null}
            <div className="grand"><span>Total TTC</span><strong>{formatMoney(data && !draft ? data.total_ttc : totals.ht + totals.vat)}</strong></div>
            {data && data.amount_paid ? <div><span>Déjà payé</span><strong>{formatMoney(data.amount_paid)}</strong></div> : null}
            {data && data.kind === "FACTURE" && !draft ? <div className="grand"><span>Reste à payer</span><strong>{formatMoney(data.amount_due)}</strong></div> : null}
          </div>
          {draft ? <p className="muted small">Les totaux définitifs sont recalculés par le serveur à l'enregistrement.{settings.vat_regime !== "ASSUJETTI" ? (withVat ? " TVA appliquée : la mention d'exonération ne sera pas imprimée." : " Régime : franchise en base de TVA, mention d'exonération imprimée (Paramètres > Entreprise). Les produits se gèrent dans Paramètres > Produits.") : ""}</p> : null}
        </Card>
        {data && !draft && settings.einvoicing_enabled ? <EinvoiceCard invoice={data} platform={settings.einvoicing_platform} onUpdated={(inv) => apply(inv, "Suivi de transmission enregistré")} onError={onError} /> : null}
        <div className="grid-2 cards">
          <Card title="Conditions et notes">
            <TextAreaField label="Conditions de paiement" disabled={!draft} value={head.payment_terms} onChange={(e) => setHead({ ...head, payment_terms: e.target.value })} />
            <TextAreaField label="Notes (imprimées sur la facture)" disabled={!draft} value={head.notes} onChange={(e) => setHead({ ...head, notes: e.target.value })} />
          </Card>
          {data && data.kind === "FACTURE" && !draft ? (
            <Card title="Paiements">
              <DataTable
                columns={[
                  { key: "d", header: "Date", render: (p) => formatDate(p.payment_date) },
                  { key: "m", header: "Mode", render: (p) => p.method.toLowerCase() },
                  { key: "r", header: "Référence", render: (p) => p.reference },
                  { key: "a", header: "Montant", align: "right", render: (p) => formatMoney(p.amount) },
                  { key: "x", header: "", align: "right", render: (p) => (data.status !== "ANNULEE" ? <Button variant="ghost" className="text-danger" onClick={() => delPayment.mutate(p.id)}>Supprimer</Button> : null) },
                ]}
                rows={data.payments ?? []}
                rowKey={(p) => p.id}
                empty="Aucun paiement"
              />
            </Card>
          ) : null}
        </div>
      </div>
      <ConfirmDialog open={confirm === "validate"} title="Valider la facture"
        message="Un numéro définitif va être attribué et la facture ne pourra plus être modifiée (correction uniquement par avoir). Continuer ?"
        confirmLabel="Valider" loading={action.isPending} onConfirm={() => action.mutate("validate")} onCancel={() => setConfirm(null)} />
      <ConfirmDialog open={confirm === "delete"} title="Supprimer le brouillon" message="Les temps et frais liés redeviendront facturables." confirmLabel="Supprimer" danger
        loading={remove.isPending} onConfirm={() => remove.mutate()} onCancel={() => setConfirm(null)} />
      <Modal open={!!pay} title="Enregistrer un paiement" onClose={() => setPay(null)}
        footer={<><Button onClick={() => setPay(null)}>Annuler</Button><Button variant="primary" loading={addPayment.isPending} onClick={() => addPayment.mutate()}>Enregistrer</Button></>}>
        {pay ? (
          <div className="grid-2">
            <MoneyField label="Montant" value={pay.amount} onChange={(v) => setPay({ ...pay, amount: v })} />
            <TextField label="Date" type="date" value={pay.payment_date} onChange={(e) => setPay({ ...pay, payment_date: e.target.value })} />
            <SelectField label="Mode" value={pay.method} onChange={(e) => setPay({ ...pay, method: e.target.value })}
              options={[["VIREMENT", "Virement"], ["CHEQUE", "Chèque"], ["CARTE", "Carte"], ["ESPECES", "Espèces"], ["PRELEVEMENT", "Prélèvement"], ["AUTRE", "Autre"]].map(([value, label]) => ({ value, label }))} />
            <TextField label="Référence" value={pay.reference} onChange={(e) => setPay({ ...pay, reference: e.target.value })} />
          </div>
        ) : null}
      </Modal>
      <Modal open={cancel !== null} title="Annuler la facture par un avoir" onClose={() => setCancel(null)}
        footer={<><Button onClick={() => setCancel(null)}>Retour</Button><Button variant="danger" loading={creditNote.isPending} onClick={() => creditNote.mutate()}>Créer l'avoir</Button></>}>
        <div className="form-stack">
          <p>Un avoir du même montant sera créé et validé. La facture passera « Annulée » et ses temps et frais redeviendront facturables.</p>
          <TextField label="Motif" value={cancel ?? ""} onChange={(e) => setCancel(e.target.value)} />
        </div>
      </Modal>
    </>
  );
}

/** Lot 6 : génération d'une facture depuis les temps et frais non facturés d'un projet ou d'une mission. */
// Bloc "Facturation électronique" d'une facture validée : fichiers à déposer sur la plateforme agréée et suivi de transmission.
function EinvoiceCard({ invoice, platform, onUpdated, onError }: { invoice: Invoice; platform: string; onUpdated: (inv: Invoice) => void; onError: (e: unknown) => void }) {
  const [status, setStatus] = useState<EinvoiceStatus>(invoice.einvoice_status ?? "A_TRANSMETTRE");
  const [reference, setReference] = useState(invoice.einvoice_ref);
  const [message, setMessage] = useState(invoice.einvoice_message);
  useEffect(() => { setStatus(invoice.einvoice_status ?? "A_TRANSMETTRE"); setReference(invoice.einvoice_ref); setMessage(invoice.einvoice_message); }, [invoice]);
  const save = useMutation({
    mutationFn: () => api.post<Invoice>(`/invoices/${invoice.id}/einvoice`, { status, reference, message }),
    onSuccess: onUpdated,
    onError,
  });
  const st = invoice.einvoice_status ? EINVOICE_STATUS[invoice.einvoice_status] : null;
  const b2c = invoice.einvoice_status === "HORS_CHAMP";
  return (
    <Card title="Facturation électronique">
      <div className="grid-3">
        <div className="field"><label>Statut de transmission</label><div className="readonly">{st ? <Badge tone={st.tone}>{st.label}</Badge> : <Badge>Non suivie (émise avant l'activation)</Badge>}{invoice.einvoice_updated_at ? <span className="muted small"> le {formatDateTime(invoice.einvoice_updated_at)}</span> : null}</div></div>
        <div className="field"><label>SIREN du client (figé)</label><div className="readonly">{invoice.billing_siren || <span className="muted">non renseigné</span>}</div></div>
        <div className="field"><label>Catégorie d'opération</label><div className="readonly">{OPERATION_CATEGORY_LABELS[invoice.operation_category]}</div></div>
      </div>
      {b2c ? (
        <p className="muted small">Client particulier : cette facture n'entre pas dans l'e-invoicing (elle relève du e-reporting des transactions B2C, transmis par votre plateforme agréée à partir des données de facturation et d'encaissement).</p>
      ) : (
        <>
          <div className="inline-form">
            <a className="btn btn-secondary" href={`/api/invoices/${invoice.id}/pdf`} target="_blank" rel="noreferrer">PDF Factur-X</a>
            <a className="btn" href={`/api/invoices/${invoice.id}/xml?format=cii`} download>XML CII (Factur-X)</a>
            <a className="btn" href={`/api/invoices/${invoice.id}/xml?format=ubl`} download>XML UBL</a>
            <span className="muted small">À déposer sur {platform || "votre plateforme agréée"} ; le PDF Factur-X contient le XML CII.</span>
          </div>
          <div className="grid-3">
            <SelectField label="Nouveau statut" value={status} onChange={(e) => setStatus(e.target.value as EinvoiceStatus)}
              options={(["A_TRANSMETTRE", "TRANSMISE", "ACCEPTEE", "REFUSEE"] as EinvoiceStatus[]).map((v) => ({ value: v, label: EINVOICE_STATUS[v].label }))} />
            <TextField label="Référence plateforme" value={reference} onChange={(e) => setReference(e.target.value)} placeholder="identifiant donné par la PA" />
            <TextField label="Message (motif de refus, remarque)" value={message} onChange={(e) => setMessage(e.target.value)} />
          </div>
          <div className="form-actions"><Button variant="primary" loading={save.isPending} onClick={() => save.mutate()}>Enregistrer le suivi</Button></div>
        </>
      )}
    </Card>
  );
}

// Facturer depuis un projet : sélection des temps et frais en attente, forfait éventuel.
export function BillFromProjectPage() {
  const { projectId } = useParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({ queryKey: ["billable", projectId], queryFn: () => api.get<Billable>(`/billing/projects/${projectId}`) });
  const [ts, setTs] = useState<number[] | null>(null);
  const [ex, setEx] = useState<number[] | null>(null);
  const [flat, setFlat] = useState(true);
  const [date, setDate] = useState(todayIso());
  const [error, setError] = useState("");
  useEffect(() => {
    if (data && ts === null) {
      setTs(data.timesheets.map((t) => t.id));
      setEx(data.expenses.map((e) => e.id));
    }
  }, [data, ts]);
  // Crée le brouillon puis l'ouvre.
  const create = useMutation({
    mutationFn: () => api.post<Invoice>("/billing/from-timesheets", { project_id: Number(projectId), timesheet_ids: ts, expense_ids: ex, include_flat_fee: flat, invoice_date: date }),
    onSuccess: (inv) => {
      qc.invalidateQueries({ queryKey: ["invoices"] });
      qc.invalidateQueries({ queryKey: ["mission"] });
      qc.invalidateQueries({ queryKey: ["missions"] });
      qc.invalidateQueries({ queryKey: ["timesheets"] });
      qc.invalidateQueries({ queryKey: ["project"] });
      toast("Facture brouillon créée");
      nav(`/facturation/${inv.id}`);
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });
  if (isLoading || !data || ts === null || ex === null) return <Loading />;
  const total = data.timesheets.filter((t) => ts.includes(t.id)).reduce((s, t) => s + t.amount, 0) + data.expenses.filter((e) => ex.includes(e.id)).reduce((s, e) => s + e.amount, 0) + (flat ? data.flat_fee_pending : 0);
  const minutes = data.timesheets.filter((t) => ts.includes(t.id)).reduce((s, t) => s + t.duration_minutes, 0);
  return (
    <>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb"><Link to="/facturation">Factures</Link><span>Facturer : {data.project_name}</span></div>
        </div>
        <div className="page-header-right">
          <TextField label="Date de facture" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          <Button variant="primary" loading={create.isPending} disabled={total <= 0} onClick={() => create.mutate()}>Générer la facture ({formatMoney(total)})</Button>
        </div>
      </div>
      <div className="page-body wide">
        <Alert>{error}</Alert>
        <p className="muted">Client : <strong>{data.client_name}</strong>. Les éléments cochés seront regroupés dans une facture brouillon et ne pourront plus être facturés une seconde fois.</p>
        {data.flat_fee_pending ? (
          <Card title="Forfait">
            <Checkbox label={`Forfait de la mission : ${formatMoney(data.flat_fee_pending)}`} checked={flat} onChange={(e) => setFlat(e.target.checked)} />
          </Card>
        ) : null}
        <Card title={`Temps (${formatMinutes(minutes)})`}>
          <DataTable
            columns={[
              { key: "c", header: <input type="checkbox" aria-label="Tout" checked={ts.length === data.timesheets.length} onChange={(e) => setTs(e.target.checked ? data.timesheets.map((t) => t.id) : [])} />,
                render: (t) => <input type="checkbox" aria-label="Facturer" checked={ts.includes(t.id)} onChange={(e) => setTs(e.target.checked ? [...ts, t.id] : ts.filter((x) => x !== t.id))} /> },
              { key: "d", header: "Date", render: (t) => formatDate(t.date) },
              { key: "u", header: "Intervenant", render: (t) => t.user_name },
              { key: "t", header: "Tâche", render: (t) => t.task_name ?? "" },
              { key: "x", header: "Description", render: (t) => t.description },
              { key: "m", header: "Durée", align: "right", render: (t) => formatMinutes(t.duration_minutes) },
              { key: "r", header: "Taux", align: "right", render: (t) => `${formatMoney(t.hourly_rate)}/h` },
              { key: "a", header: "Montant", align: "right", render: (t) => formatMoney(t.amount) },
            ]}
            rows={data.timesheets}
            rowKey={(t) => t.id}
            empty="Aucun temps facturable en attente"
          />
        </Card>
        {data.expenses.length ? (
          <Card title="Frais">
            <DataTable
              columns={[
                { key: "c", header: "", render: (e) => <input type="checkbox" aria-label="Facturer" checked={ex.includes(e.id)} onChange={(x) => setEx(x.target.checked ? [...ex, e.id] : ex.filter((y) => y !== e.id))} /> },
                { key: "k", header: "Type", render: (e) => EXPENSE_LABELS[e.kind] },
                { key: "l", header: "Libellé", render: (e) => e.label },
                { key: "q", header: "Quantité", align: "right", render: (e) => formatQty(e.quantity_milli) },
                { key: "a", header: "Montant", align: "right", render: (e) => formatMoney(e.amount) },
              ]}
              rows={data.expenses}
              rowKey={(e) => e.id}
            />
          </Card>
        ) : null}
      </div>
    </>
  );
}

// Projet avec un montant à facturer.
interface PendingProject {
  id: number;
  name: string;
  client_name: string;
  amount_uninvoiced: number;
  minutes_billable: number;
  mission_id: number | null;
  kind: string;
}

/** Vue d'ensemble des projets et missions ayant des éléments à facturer. */
// Liste de tout ce qui reste à facturer, projet par projet.
export function ToInvoicePage() {
  const nav = useNavigate();
  const { data: projects } = useQuery({ queryKey: ["projects", "a-facturer"], queryFn: () => api.get<{ items: PendingProject[] }>("/projects?kind=&archived=tous&per_page=500") });
  const { data: missions } = useQuery({ queryKey: ["missions", "a-facturer"], queryFn: () => api.get<{ items: { project_id: number; uninvoiced_amount: number }[] }>("/missions?per_page=1000&archived=tous") });
  const missionAmounts = new Map((missions?.items ?? []).map((m) => [m.project_id, m.uninvoiced_amount]));
  const rows = (projects?.items ?? [])
    .map((p) => ({ ...p, todo: p.mission_id ? missionAmounts.get(p.id) ?? 0 : p.amount_uninvoiced }))
    .filter((p) => p.todo > 0);
  return (
    <>
      <div className="page-header">
        <div className="page-header-left"><div className="breadcrumb"><Link to="/facturation">Factures</Link><span>À facturer</span></div></div>
      </div>
      <div className="page-body wide">
        <DataTable
          columns={[
            { key: "c", header: "Client", render: (p) => p.client_name, sortValue: (p) => p.client_name },
            { key: "n", header: "Projet / mission", render: (p) => <>{p.name} {p.mission_id ? <Badge tone="info">Formation</Badge> : null}</> },
            { key: "a", header: "À facturer", align: "right", render: (p) => <strong>{formatMoney(p.todo)}</strong>, sortValue: (p) => p.todo },
            { key: "x", header: "", align: "right", render: (p) => <Button variant="primary" onClick={() => nav(`/facturation/depuis-projet/${p.id}`)}>Facturer</Button> },
          ]}
          rows={rows}
          rowKey={(p) => p.id}
          empty="Rien à facturer pour le moment"
        />
      </div>
    </>
  );
}
