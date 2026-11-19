import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, ApiError } from "../../api/client";
import type { Product } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { MoneyField } from "../../components/Fields";
import { Alert, Badge, Button, Card, Modal, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { bpToPercent, formatMoney, percentToBp } from "../../lib/format";

// Produit en cours d'édition (taux de TVA saisi en pourcentage texte).
interface Edit { id?: number; name: string; description: string; unit: string; unit_price: number; vat: string; position: string }
const EMPTY: Edit = { name: "", description: "", unit: "h", unit_price: 0, vat: "0", position: "" };

// Paramètres > Produits : liste des prestations qui pré-remplissent les lignes de facture.
export function ProductsPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: ["products"], queryFn: () => api.get<Product[]>("/products") });
  const [edit, setEdit] = useState<Edit | null>(null);
  const [error, setError] = useState("");
  const refresh = () => qc.invalidateQueries({ queryKey: ["products"] });
  const onError = (e: unknown) => setError(e instanceof ApiError ? e.message : "Erreur inattendue");

  // Création (POST) ou modification (PATCH).
  const save = useMutation({
    mutationFn: (e: Edit) => {
      const vat = percentToBp(e.vat || "0");
      if (vat === null) throw new ApiError(400, "Taux de TVA invalide (ex. 20 ou 5,5)");
      const body = { name: e.name, description: e.description, unit: e.unit, unit_price: e.unit_price, vat_rate_bp: vat, position: e.position ? Number(e.position) : undefined };
      return e.id ? api.patch<Product>(`/products/${e.id}`, body) : api.post<Product>("/products", body);
    },
    onSuccess: () => { setEdit(null); setError(""); refresh(); toast("Produit enregistré"); },
    onError,
  });
  // Archivage / réactivation.
  const toggle = useMutation({
    mutationFn: (p: Product) => api.patch<Product>(`/products/${p.id}`, { active: !p.active }),
    onSuccess: refresh,
    onError: (e) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"),
  });

  const columns: Column<Product>[] = [
    { key: "name", header: "Produit", render: (p) => <><strong>{p.name}</strong>{p.description && p.description !== p.name ? <div className="muted small">{p.description}</div> : null}</> },
    { key: "unit", header: "Unité", render: (p) => p.unit },
    { key: "price", header: "Prix unitaire HT", align: "right", render: (p) => formatMoney(p.unit_price), sortValue: (p) => p.unit_price },
    { key: "vat", header: "TVA", align: "right", render: (p) => `${bpToPercent(p.vat_rate_bp)} %` },
    { key: "active", header: "Statut", render: (p) => (p.active ? <Badge tone="success">Actif</Badge> : <Badge>Archivé</Badge>) },
    { key: "actions", header: "", align: "right", render: (p) => (
      <>
        <Button variant="ghost" onClick={() => { setError(""); setEdit({ id: p.id, name: p.name, description: p.description, unit: p.unit, unit_price: p.unit_price, vat: bpToPercent(p.vat_rate_bp), position: String(p.position) }); }}>Modifier</Button>
        <Button variant="ghost" onClick={() => toggle.mutate(p)}>{p.active ? "Archiver" : "Réactiver"}</Button>
      </>
    ) },
  ];

  return (
    <Card title="Produits et prestations" actions={<Button variant="primary" onClick={() => { setError(""); setEdit(EMPTY); }}>Nouveau produit</Button>}>
      <p className="muted">
        Dans une facture, choisir un produit sur une ligne remplit automatiquement le libellé, l'unité, le prix unitaire HT et le taux de TVA ;
        tout reste modifiable ligne par ligne. Un produit archivé n'est plus proposé mais les factures existantes le gardent.
      </p>
      <DataTable columns={columns} rows={data ?? []} rowKey={(p) => p.id} empty="Aucun produit" />
      {edit ? (
        <Modal open title={edit.id ? "Modifier le produit" : "Nouveau produit"} onClose={() => setEdit(null)}
          footer={<><Button onClick={() => setEdit(null)}>Annuler</Button><Button variant="primary" loading={save.isPending} onClick={() => save.mutate(edit)}>Enregistrer</Button></>}>
          <div className="form-stack">
            <Alert>{error}</Alert>
            <TextField label="Nom" required value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} placeholder="ex. Heure de formation" />
            <TextField label="Libellé sur la facture" value={edit.description} onChange={(e) => setEdit({ ...edit, description: e.target.value })} hint="Vide = le nom" />
            <div className="grid-2">
              <TextField label="Unité" value={edit.unit} onChange={(e) => setEdit({ ...edit, unit: e.target.value })} placeholder="h, jour, u..." />
              <MoneyField label="Prix unitaire HT" value={edit.unit_price} onChange={(v) => setEdit({ ...edit, unit_price: v })} />
              <TextField label="TVA (%)" value={edit.vat} onChange={(e) => setEdit({ ...edit, vat: e.target.value })} hint="0 en franchise ; 20 pour appliquer la TVA" />
              <TextField label="Ordre d'affichage" type="number" value={edit.position} onChange={(e) => setEdit({ ...edit, position: e.target.value })} />
            </div>
          </div>
        </Modal>
      ) : null}
    </Card>
  );
}
