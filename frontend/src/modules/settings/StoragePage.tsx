import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import React, { useEffect, useState } from "react";
import { api, ApiError } from "../../api/client";
import { Alert, Button, Card, Checkbox, ConfirmDialog, Loading, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { formatBytes } from "../../lib/format";

interface StorageInfo {
  current_dir: string;
  default_dir: string;
  config_path: string;
  configured_dir: string | null;
  shared: boolean;
  db_size: number;
  documents_count: number;
  restart_required: boolean;
}

interface Result {
  data_dir: string;
  shared: boolean;
  copied: boolean;
  existing_db: boolean;
  restart_required: boolean;
}

interface NetworkInfo {
  lan: boolean;
  port: number;
  active_lan: boolean;
  active_port: number;
  hostname: string;
  ips: string[];
}

// Option 2 : accès depuis les autres PC (lecture/écriture de /settings/network, redémarrage requis).
function NetworkCard() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: ["network"], queryFn: () => api.get<NetworkInfo>("/settings/network") });
  const [lan, setLan] = useState(false);
  const [port, setPort] = useState("8080");
  const [error, setError] = useState("");
  const [restart, setRestart] = useState(false);
  useEffect(() => {
    if (data) {
      setLan(data.lan);
      setPort(String(data.port));
    }
  }, [data]);
  // Enregistre lan + port dans erp.config.json.
  const save = useMutation({
    mutationFn: () => api.put<{ restart_required: boolean }>("/settings/network", { lan, port: Number(port) }),
    onSuccess: (r) => {
      setError("");
      setRestart(r.restart_required);
      qc.invalidateQueries({ queryKey: ["network"] });
      toast("Réglage réseau enregistré");
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });
  if (!data) return null;
  const changed = lan !== data.lan || Number(port) !== data.port;
  const addresses = [data.hostname ? `http://${data.hostname}:${port}` : "", ...data.ips.map((ip) => `http://${ip}:${port}`)].filter(Boolean);
  return (
    <Card title="Option 2 : accès depuis les autres PC du réseau (ce PC reste allumé)">
      <p>
        Mercure reste lancé sur ce PC et les autres postes l'ouvrent dans leur navigateur. Une seule base, aucune contrainte de verrou : c'est la solution la plus sûre
        quand ce PC est allumé pendant que les autres travaillent.
      </p>
      {data.active_lan ? <Alert tone="success">Accès réseau actif. Adresses à utiliser sur les autres PC : {addresses.map((a) => <code key={a}>{a}</code>).reduce<React.ReactNode[]>((acc, el, i) => (i ? [...acc, " ou ", el] : [el]), [])}</Alert>
        : <Alert tone="info">Accès réseau inactif : Mercure ne répond qu'à ce PC.</Alert>}
      <Alert>{error}</Alert>
      {restart ? <Alert tone="warning">Fermez Mercure (fenêtre noire) puis relancez mercure.exe pour appliquer. Au premier lancement, Windows demandera d'autoriser mercure.exe dans le pare-feu : acceptez pour le réseau privé.</Alert> : null}
      <Checkbox label="Autoriser les autres PC du réseau local à utiliser Mercure" checked={lan} onChange={(e) => setLan(e.target.checked)} />
      <TextField label="Port" type="number" min={1024} max={65535} value={port} onChange={(e) => setPort(e.target.value)} hint="8080 par défaut" />
      <p className="muted small">
        À réserver à un réseau local de confiance (box ou réseau d'entreprise) : la connexion n'est pas chiffrée. Chaque personne se connecte avec son propre compte
        (Paramètres &gt; Utilisateurs).
      </p>
      <div className="form-actions">
        <Button variant="primary" disabled={!changed} loading={save.isPending} onClick={() => save.mutate()}>Enregistrer</Button>
      </div>
    </Card>
  );
}

// Option 1 : emplacement du dossier de données (copie assistée, mode partagé).
export function StoragePage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: ["storage"], queryFn: () => api.get<StorageInfo>("/settings/storage") });
  const [dir, setDir] = useState("");
  const [shared, setShared] = useState(false);
  const [copy, setCopy] = useState(true);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [result, setResult] = useState<Result | null>(null);

  useEffect(() => {
    if (data) {
      setDir(data.configured_dir ?? "");
      setShared(data.shared);
    }
  }, [data]);

  const save = useMutation({
    mutationFn: () => api.put<Result>("/settings/storage", { data_dir: dir, shared, copy_data: copy }),
    onSuccess: (r) => {
      setConfirm(false);
      setResult(r);
      setError("");
      qc.invalidateQueries({ queryKey: ["storage"] });
      toast("Emplacement enregistré");
    },
    onError: (e) => {
      setConfirm(false);
      setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    },
  });

  if (!data) return <Loading />;
  const looksShared = /^\\\\|onedrive|dropbox/i.test(dir);
  const changed = (dir || null) !== data.configured_dir || shared !== data.shared;

  return (
    <div className="form-stack">
      {data.restart_required ? <Alert tone="info">Un nouvel emplacement est enregistré : il sera utilisé au prochain démarrage de Mercure.</Alert> : null}
      <Card title="Emplacement actuel">
        <dl className="details wide-details">
          <dt>Dossier utilisé</dt><dd><code>{data.current_dir}</code></dd>
          <dt>Base de données</dt><dd>erp.db ({formatBytes(data.db_size)})</dd>
          <dt>Documents</dt><dd>{data.documents_count} fichier(s)</dd>
          <dt>Mode</dt><dd>{data.shared ? "dossier partagé (journal classique)" : "dossier local"}</dd>
          <dt>Fichier de configuration</dt><dd><code>{data.config_path}</code></dd>
        </dl>
      </Card>

      <Card title="Option 1 : dossier de données partagé (un PC à la fois)">
        <p>
          Placez le dossier de données sur un emplacement accessible aux deux postes (disque réseau, NAS, dossier partagé Windows ou clé USB), puis indiquez ce chemin sur
          chaque PC. Le fichier <code>mercure.exe</code> peut être copié tel quel sur l'autre PC.
        </p>
        <Alert tone="warning">
          N'ouvrez Mercure que sur un seul PC à la fois. SQLite protège la base par des verrous, mais deux instances simultanées sur un partage réseau peuvent se gêner.
          Évitez les dossiers synchronisés par OneDrive, Dropbox ou Google Drive pendant l'utilisation : la synchronisation peut corrompre la base. Vérifiez régulièrement
          les sauvegardes (Paramètres &gt; Sauvegardes).
        </Alert>
        <Alert>{error}</Alert>
        <TextField label="Dossier des données" value={dir} onChange={(e) => setDir(e.target.value)} placeholder={data.default_dir}
          hint={`Laisser vide pour le dossier par défaut (${data.default_dir}). Exemples : D:\\Mercure\\data, \\\\NAS\\partage\\erp, E:\\erp-data`} wide />
        <Checkbox label="Dossier partagé ou réseau (recommandé pour un disque réseau, un NAS ou une clé USB)" checked={shared || looksShared} disabled={looksShared}
          onChange={(e) => setShared(e.target.checked)} />
        <Checkbox label="Copier les données actuelles (base, documents, sauvegardes) dans le nouveau dossier s'il est vide" checked={copy} onChange={(e) => setCopy(e.target.checked)} />
        <p className="muted small">
          Si le nouveau dossier contient déjà une base <code>erp.db</code> (par exemple créée depuis l'autre PC), décochez la copie : Mercure utilisera ces données existantes.
        </p>
        <div className="form-actions">
          <Button variant="primary" disabled={!changed} onClick={() => setConfirm(true)}>Enregistrer l'emplacement</Button>
        </div>
      </Card>

      {result ? (
        <Card title="Étape suivante">
          <ol className="steps-list">
            {result.copied ? <li>Les données ont été copiées dans <code>{result.data_dir}</code>. Les anciennes restent en place (vous pourrez les supprimer plus tard).</li> : null}
            {result.existing_db ? <li>Le dossier <code>{result.data_dir}</code> contient déjà une base : elle sera utilisée telle quelle.</li> : null}
            {result.restart_required ? <li><strong>Fermez Mercure (fenêtre noire) puis relancez mercure.exe</strong> pour travailler sur le nouvel emplacement.</li> : null}
            <li>Sur l'autre PC : copiez <code>mercure.exe</code>, lancez-le une fois, puis renseignez le même dossier ici (ou copiez aussi <code>erp.config.json</code> à côté de l'exe).</li>
          </ol>
        </Card>
      ) : null}

      <NetworkCard />

      <ConfirmDialog open={confirm} title="Changer l'emplacement des données"
        message={`Le dossier ${dir || data.default_dir} sera utilisé au prochain démarrage.${copy ? " Les données actuelles y seront copiées s'il est vide." : ""} Continuer ?`}
        confirmLabel="Enregistrer" loading={save.isPending} onConfirm={() => save.mutate()} onCancel={() => setConfirm(false)} />
    </div>
  );
}
