import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../../api/client";
import { Alert, Button, Card } from "../../components/ui";
import { useToast } from "../../components/Toast";

// Chargement / suppression du jeu de démonstration (ADMIN).
export function DemoPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: ["demo"], queryFn: () => api.get<{ loaded: boolean }>("/demo") });
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");
  const done = (msg: string) => {
    qc.invalidateQueries();
    toast(msg);
  };
  const load = useMutation({ mutationFn: () => api.post("/demo"), onSuccess: () => done("Données de démonstration chargées"), onError });
  const remove = useMutation({ mutationFn: () => api.del("/demo"), onSuccess: () => done("Données de démonstration supprimées"), onError });
  return (
    <Card title="Données de démonstration">
      <p className="muted">
        Jeu d'essai fictif : société « Demo Industrie SAS (FICTIF) », contact « MARTIN (FICTIF) Jean », projet « Formation Microsoft 365 (démo) », 7 h à 80 € et une facture
        brouillon de 560 € HT. Il se supprime en un clic tant que la facture de démonstration n'a pas été validée.
      </p>
      {data?.loaded ? (
        <>
          <Alert tone="info">Les données de démonstration sont chargées.</Alert>
          <div><Button variant="danger" loading={remove.isPending} onClick={() => remove.mutate()}>Supprimer les données de démonstration</Button></div>
        </>
      ) : (
        <div><Button variant="primary" loading={load.isPending} onClick={() => load.mutate()}>Charger les données de démonstration</Button></div>
      )}
    </Card>
  );
}
