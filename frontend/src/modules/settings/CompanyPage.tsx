import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useEffect, useState } from "react";
import { api, ApiError } from "../../api/client";
import type { CompanySettings } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { Alert, Button, Card, Checkbox, Loading, SelectField, TextAreaField, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { bpToPercent, percentToBp } from "../../lib/format";

type Form = Omit<CompanySettings, "default_vat_rate_bp" | "default_due_days" | "invoice_number_width"> & {
  vat_rate: string;
  default_due_days: string;
  invoice_number_width: string;
};

// Paramètres serveur -> formulaire (taux de TVA en pourcentage texte).
function toForm(s: CompanySettings): Form {
  return {
    ...s,
    vat_rate: bpToPercent(s.default_vat_rate_bp),
    default_due_days: String(s.default_due_days),
    invoice_number_width: String(s.invoice_number_width),
  };
}

// Identité de l'entreprise, banque, TVA, mentions et numérotation ; envoi du logo.
export function CompanyPage() {
  const { user } = useAuth();
  const canEdit = hasRole(user, "ADMIN");
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({ queryKey: ["settings"], queryFn: () => api.get<CompanySettings>("/settings/company") });
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState("");
  // Change l'URL du logo après envoi pour contourner le cache du navigateur.
  const [logoVersion, setLogoVersion] = useState(Date.now());

  useEffect(() => {
    if (data) setForm(toForm(data));
  }, [data]);

  // Enregistrement (le serveur valide SIRET, IBAN, NDA...).
  const save = useMutation({
    mutationFn: (body: CompanySettings) => api.put<CompanySettings>("/settings/company", body),
    onSuccess: (s) => {
      qc.setQueryData(["settings"], s);
      setError("");
      toast("Paramètres enregistrés");
    },
    onError: (e) => {
      setError(e instanceof ApiError ? e.message : "Erreur inattendue");
      window.scrollTo({ top: 0, behavior: "smooth" });
    },
  });

  // Envoi du logo en multipart.
  const uploadLogo = useMutation({
    mutationFn: (file: File) => {
      const fd = new FormData();
      fd.append("logo", file);
      return api.post<CompanySettings>("/settings/company/logo", fd);
    },
    onSuccess: (s) => {
      qc.setQueryData(["settings"], s);
      setLogoVersion(Date.now());
      toast("Logo enregistré");
    },
    onError: (e) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"),
  });

  if (isLoading || !form) return <Loading />;

  const set = (k: keyof Form) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) =>
    setForm({ ...form, [k]: e.target.value });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const bp = form.vat_regime === "FRANCHISE" ? 0 : percentToBp(form.vat_rate);
    if (bp === null || bp > 10000) {
      setError("Taux de TVA invalide (ex. 20 ou 5,5)");
      window.scrollTo({ top: 0, behavior: "smooth" });
      return;
    }
    const { vat_rate: _ignored, ...rest } = form;
    save.mutate({
      ...rest,
      default_vat_rate_bp: bp,
      default_due_days: Number(form.default_due_days),
      invoice_number_width: Number(form.invoice_number_width),
    });
  };

  const example = `${form.invoice_prefix || "FAC"}-${new Date().getFullYear()}-${"1".padStart(Number(form.invoice_number_width) || 6, "0")}`;

  return (
    <form onSubmit={submit} className="form-stack">
      <Alert>{error}</Alert>
      {!canEdit ? <Alert tone="info">Consultation seule : seul un administrateur peut modifier ces informations.</Alert> : null}
      <fieldset disabled={!canEdit} className="form-stack plain">
        <Card title="Identité">
          <div className="grid-2">
            <TextField label="Raison sociale" required value={form.legal_name} onChange={set("legal_name")} wide />
            <TextField label="Adresse" value={form.address_line1} onChange={set("address_line1")} />
            <TextField label="Complément d'adresse" value={form.address_line2} onChange={set("address_line2")} />
            <TextField label="Code postal" value={form.zip} onChange={set("zip")} />
            <TextField label="Ville" value={form.city} onChange={set("city")} />
            <TextField label="Pays" value={form.country} onChange={set("country")} />
            <TextField label="SIREN" value={form.siren} onChange={set("siren")} inputMode="numeric" />
            <TextField label="SIRET" value={form.siret} onChange={set("siret")} inputMode="numeric" />
            <TextField label="N° TVA intracommunautaire" value={form.vat_number} onChange={set("vat_number")} />
            <TextField label="NDA (n° de déclaration d'activité)" value={form.nda} onChange={set("nda")} inputMode="numeric" hint="11 chiffres, attribué par la DREETS" />
            <TextField label="Email" type="email" value={form.email} onChange={set("email")} />
            <TextField label="Téléphone" value={form.phone} onChange={set("phone")} />
            <TextField label="Site web" value={form.website} onChange={set("website")} />
          </div>
        </Card>

        <Card title="Logo">
          <div className="logo-row">
            {data?.logo_file ? <img className="logo-preview" src={`/api/settings/company/logo?v=${logoVersion}`} alt="Logo de l'entreprise" /> : <span className="muted">Aucun logo</span>}
            {canEdit ? (
              <label className="btn btn-secondary">
                {uploadLogo.isPending ? "Envoi..." : "Choisir un fichier (PNG ou JPEG, 2 Mo max)"}
                <input type="file" accept="image/png,image/jpeg" hidden onChange={(e) => e.target.files?.[0] && uploadLogo.mutate(e.target.files[0])} />
              </label>
            ) : null}
          </div>
        </Card>

        <Card title="Banque et paiement">
          <div className="grid-2">
            <TextField label="Banque" value={form.bank_name} onChange={set("bank_name")} />
            <TextField label="IBAN" value={form.iban} onChange={set("iban")} />
            <TextField label="BIC" value={form.bic} onChange={set("bic")} />
            <TextField label="Délai d'échéance par défaut (jours)" type="number" min={0} max={365} value={form.default_due_days} onChange={set("default_due_days")} />
            <TextAreaField label="Conditions de paiement" value={form.payment_terms} onChange={set("payment_terms")} placeholder="ex. Paiement à 30 jours par virement" />
          </div>
        </Card>

        <Card title="TVA et mentions légales">
          <div className="grid-2">
            <SelectField
              label="Régime de TVA"
              value={form.vat_regime}
              onChange={set("vat_regime")}
              options={[
                { value: "FRANCHISE", label: "Franchise en base (pas de TVA)" },
                { value: "ASSUJETTI", label: "Assujetti à la TVA" },
              ]}
            />
            {form.vat_regime === "ASSUJETTI" ? <TextField label="Taux de TVA par défaut (%)" value={form.vat_rate} onChange={set("vat_rate")} inputMode="decimal" /> : <span />}
            <TextAreaField
              label="Mention d'exonération de TVA"
              value={form.vat_exemption_mention}
              onChange={set("vat_exemption_mention")}
              hint="Texte imprimé sur les factures en franchise. À vérifier avec votre comptable."
            />
            <TextAreaField label="Mentions légales du pied de facture" value={form.legal_mentions} onChange={set("legal_mentions")} hint="Pénalités de retard, indemnité forfaitaire de recouvrement, etc. À vérifier avec votre comptable." />
          </div>
        </Card>

        <Card title="Facturation électronique (réforme française)">
          <p className="muted small">
            Réception obligatoire pour toutes les entreprises depuis le 1er septembre 2026 ; émission obligatoire pour les TPE/PME et
            micro-entreprises au 1er septembre 2027, via une plateforme agréée (PA) immatriculée par la DGFiP. Une fois activée, Mercure
            exige le SIREN des clients professionnels à la validation, produit le PDF Factur-X (XML embarqué) et les XML CII / UBL à déposer
            sur votre plateforme, et suit le statut de transmission de chaque facture. Les factures aux particuliers sont hors champ (e-reporting).
          </p>
          <Checkbox label="Activer la facturation électronique" checked={form.einvoicing_enabled} onChange={(e) => setForm({ ...form, einvoicing_enabled: e.target.checked })} />
          <div className="grid-2">
            <TextField label="Plateforme agréée choisie" value={form.einvoicing_platform} onChange={set("einvoicing_platform")} placeholder="nom de votre PA" hint="Liste officielle sur impots.gouv.fr" />
            <SelectField label="Catégorie d'opération par défaut" value={form.default_operation_category} onChange={set("default_operation_category")}
              options={[{ value: "SERVICES", label: "Prestation de services" }, { value: "BIENS", label: "Livraison de biens" }, { value: "MIXTE", label: "Mixte" }]} />
          </div>
          <Checkbox label="Option pour le paiement de la TVA d'après les débits (mention ajoutée aux factures électroniques)" checked={form.vat_on_debits} onChange={(e) => setForm({ ...form, vat_on_debits: e.target.checked })} />
        </Card>

        <Card title="Numérotation des factures">
          <div className="grid-2">
            <TextField label="Préfixe" value={form.invoice_prefix} onChange={set("invoice_prefix")} maxLength={10} />
            <TextField label="Nombre de chiffres" type="number" min={3} max={10} value={form.invoice_number_width} onChange={set("invoice_number_width")} />
          </div>
          <p className="muted">
            Exemple : <strong>{example}</strong>. Le numéro est attribué par le serveur à la validation, sans trou ni doublon.
          </p>
        </Card>
      </fieldset>
      {canEdit ? (
        <div className="form-actions">
          <Button type="button" onClick={() => data && setForm(toForm(data))}>
            Annuler les modifications
          </Button>
          <Button variant="primary" type="submit" loading={save.isPending}>
            Enregistrer
          </Button>
        </div>
      ) : null}
    </form>
  );
}
