//! Lot 7 : génération du PDF de facture, sans dépendance externe lourde.
//! Polices standard PDF (Helvetica, encodage WinAnsi : accents français et €), logo PNG ou JPEG.
//! Les mentions légales et la mention de TVA viennent des paramètres (figées à la validation).

use std::io::Write;

use flate2::{write::ZlibEncoder, Compression};

use super::service::InvoiceDetail;
use crate::{
    domain::{money::Money, validation::parse_date},
    error::{AppError, AppResult},
    modules::settings::CompanySettings,
};

// Format A4 en points PDF (1 pt = 1/72 pouce) : largeur, hauteur, marge, réserve du pied de page.
const W: f32 = 595.0;
const H: f32 = 842.0;
const M: f32 = 42.0;
const BOTTOM: f32 = 110.0;

// Largeurs Helvetica / Helvetica-Bold (pour 1000 unités), caractères ASCII 32 à 126.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556,
    556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_B: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611,
    611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Lettre de base d'un caractère accentué (pour la largeur).
// Ramène un caractère accentué à sa base pour la mesure de largeur (les tables ne couvrent que l'ASCII).
fn base_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'ç' => 'c',
        'Ç' => 'C',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'ý' | 'ÿ' => 'y',
        '\u{a0}' | '\u{202f}' => ' ',
        _ => c,
    }
}

// Largeur d'un caractère pour 1000 unités de taille.
fn char_width(c: char, bold: bool) -> f32 {
    let table = if bold { &HELV_B } else { &HELV };
    let b = base_char(c);
    let w = match b {
        ' '..='~' => table[(b as u32 - 32) as usize],
        '€' | '«' | '»' => 556,
        'œ' => 944,
        'Œ' => 1000,
        '°' => 400,
        '’' | '‘' => 222,
        '“' | '”' => 333,
        '–' => 556,
        '—' => 1000,
        '…' => 1000,
        _ => 556,
    };
    w as f32 / 1000.0
}

// Largeur d'une chaîne à une taille donnée (pour aligner à droite et couper les lignes).
pub fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    s.chars().map(|c| char_width(c, bold)).sum::<f32>() * size
}

/// Conversion en WinAnsi + échappement PDF.
// Encode une chaîne en WinAnsi (accents, €) entre parenthèses, avec échappement des caractères spéciaux PDF.
fn pdf_string(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let b: u8 = match c {
            '€' => 0x80,
            '‚' => 0x82,
            '…' => 0x85,
            'Œ' => 0x8C,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            'œ' => 0x9C,
            '\u{202f}' => 0xA0,
            c if (c as u32) < 0x20 => b' ',
            c if (c as u32) < 0x80 => c as u8,
            c if (0xA0..=0xFF).contains(&(c as u32)) => c as u32 as u8,
            _ => b'?',
        };
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// Découpe un texte en lignes tenant dans `max` points.
// Coupe un texte en lignes qui tiennent dans `max` points (coupure aux espaces, mot trop long coupé).
pub fn wrap(text: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split(' ') {
            let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if text_width(&candidate, size, bold) <= max || cur.is_empty() {
                // Mot trop long : coupé au caractère.
                if text_width(&candidate, size, bold) > max && cur.is_empty() {
                    let mut piece = String::new();
                    for ch in word.chars() {
                        if text_width(&format!("{piece}{ch}"), size, bold) > max && !piece.is_empty() {
                            lines.push(std::mem::take(&mut piece));
                        }
                        piece.push(ch);
                    }
                    cur = piece;
                } else {
                    cur = candidate;
                }
            } else {
                lines.push(std::mem::replace(&mut cur, word.to_string()));
            }
        }
        lines.push(cur);
    }
    lines
}

// Logo décodé : dimensions, composantes, octets compressés, masque alpha éventuel.
struct Image {
    width: u32,
    height: u32,
    /// (dictionnaire supplémentaire, données) pour l'image et son masque éventuel.
    filter: &'static str,
    color_space: &'static str,
    data: Vec<u8>,
    smask: Option<Vec<u8>>,
}

// Lit largeur / hauteur / nombre de composantes dans les marqueurs SOF d'un JPEG (inséré tel quel, DCTDecode).
fn jpeg_size(data: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some((w, h, data[i + 9]));
        }
        i += 2 + len;
    }
    None
}

// Compression zlib (FlateDecode).
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

// PNG décodé en RGB + canal alpha séparé (SMask) ; JPEG embarqué directement.
fn load_image(bytes: &[u8]) -> Option<Image> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let (w, h, comps) = jpeg_size(bytes)?;
        let cs = match comps {
            1 => "/DeviceGray",
            4 => "/DeviceCMYK",
            _ => "/DeviceRGB",
        };
        return Some(Image { width: w, height: h, filter: "/DCTDecode", color_space: cs, data: bytes.to_vec(), smask: None });
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    let (rgb, alpha): (Vec<u8>, Option<Vec<u8>>) = match info.color_type {
        png::ColorType::Rgb => (px.to_vec(), None),
        png::ColorType::Rgba => (
            px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(),
            Some(px.chunks(4).map(|p| p[3]).collect()),
        ),
        png::ColorType::Grayscale => (px.iter().flat_map(|g| [*g, *g, *g]).collect(), None),
        png::ColorType::GrayscaleAlpha => (
            px.chunks(2).flat_map(|p| [p[0], p[0], p[0]]).collect(),
            Some(px.chunks(2).map(|p| p[1]).collect()),
        ),
        _ => return None,
    };
    Some(Image {
        width: info.width,
        height: info.height,
        filter: "/FlateDecode",
        color_space: "/DeviceRGB",
        data: zlib(&rgb),
        smask: alpha.map(|a| zlib(&a)),
    })
}

/// Page en cours de construction (flux de contenu).
// Flux de contenu de la page en cours + pages terminées.
struct Canvas {
    pages: Vec<Vec<u8>>,
    cur: Vec<u8>,
}

impl Canvas {
    // Nouveau document, première page ouverte.
    fn new() -> Self {
        Self { pages: Vec::new(), cur: Vec::new() }
    }
    // Ajoute des opérateurs PDF bruts.
    fn raw(&mut self, s: &str) {
        self.cur.extend_from_slice(s.as_bytes());
    }
    // Texte à la position (x, y), police normale ou grasse.
    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, s: &str) {
        self.raw(&format!("BT /{} {size} Tf {x:.2} {y:.2} Td ", if bold { "F2" } else { "F1" }));
        let enc = pdf_string(s);
        self.cur.extend_from_slice(&enc);
        self.raw(" Tj ET\n");
    }
    // Texte aligné à droite sur x_right.
    fn text_right(&mut self, x_right: f32, y: f32, size: f32, bold: bool, s: &str) {
        let w = text_width(s, size, bold);
        self.text(x_right - w, y, size, bold, s);
    }
    // Couleur de remplissage (gris pour les libellés secondaires).
    fn color(&mut self, r: f32, g: f32, b: f32) {
        self.raw(&format!("{r} {g} {b} rg {r} {g} {b} RG\n"));
    }
    // Trait horizontal ou vertical.
    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        self.raw(&format!("{width} w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n"));
    }
    // Clôt la page courante et en ouvre une nouvelle.
    fn new_page(&mut self) {
        let p = std::mem::take(&mut self.cur);
        self.pages.push(p);
    }
}

// "2026-11-18" -> "18/11/2026".
fn fr_date(d: &str) -> String {
    parse_date(d).map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_else(|| d.to_string())
}

// Montant formaté "1 234,56 €".
fn money(c: i64) -> String {
    Money(c).format_fr()
}

// Dessine la facture (mise en page calquée sur le modèle Odoo du client) et assemble le PDF.
pub fn render(d: &InvoiceDetail, cs: &CompanySettings, logo: Option<&[u8]>) -> AppResult<Vec<u8>> {
    render_with(d, cs, logo, None)
}

/// Pièce jointe Factur-X : le XML CII embarqué dans le PDF, avec les métadonnées XMP associées.
pub struct Attachment<'a> {
    pub xml: &'a [u8],
    pub xmp: &'a str,
}

/// Variante avec facture électronique embarquée (PDF hybride Factur-X).
pub fn render_with(d: &InvoiceDetail, cs: &CompanySettings, logo: Option<&[u8]>, attachment: Option<Attachment<'_>>) -> AppResult<Vec<u8>> {
    let r = &d.row;
    let is_draft = r.status == "BROUILLON";
    // Colonne et ligne de TVA affichées seulement si une TVA s'applique.
    let with_vat = d.lines.iter().any(|l| l.vat_rate_bp > 0) || cs.vat_regime == "ASSUJETTI";
    let image = logo.and_then(load_image);
    let mut c = Canvas::new();
    let mut page_no = 1;
    let number = r.number.clone().unwrap_or_else(|| "brouillon".into());
    // Titre selon le type et l'état : Facture n°, Avoir n°, ou brouillon.
    let title = match (r.kind.as_str(), is_draft) {
        ("AVOIR", _) => format!("Avoir {number}"),
        (_, true) => "Facture (brouillon)".to_string(),
        _ => format!("Facture {number}"),
    };
    let xr = W - M;
    let col2 = 312.0; // colonne droite (client, échéance)
    let dark = (0.13, 0.13, 0.13);
    let grey = (0.45, 0.45, 0.45);
    let light = (0.62, 0.62, 0.62);

    // ---- En-tête émetteur : logo + adresse courte (modèle Odoo) ----
    let mut x_text = M;
    let top = H - M;
    if let Some(img) = &image {
        let scale = (64.0 / img.height as f32).min(90.0 / img.width as f32);
        let (w, h) = (img.width as f32 * scale, img.height as f32 * scale);
        c.raw(&format!("q {w:.2} 0 0 {h:.2} {M:.2} {:.2} cm /Im1 Do Q\n", top - h));
        x_text = M + w + 12.0;
    }
    c.color(dark.0, dark.1, dark.2);
    let mut y = top - 12.0;
    let mut emitter: Vec<String> = vec![cs.address_line1.clone(), cs.address_line2.clone(), format!("{} {}", cs.zip, cs.city).trim().to_string()];
    if !cs.vat_number.is_empty() {
        emitter.push(format!("TVA: {}", cs.vat_number));
    }
    if image.is_none() {
        c.text(M, y, 12.0, true, &cs.legal_name);
        y -= 15.0;
    }
    for l in emitter.iter().filter(|l| !l.is_empty()) {
        c.text(x_text, y, 9.5, false, l);
        y -= 13.0;
    }

    // ---- Client (colonne droite, plus bas comme sur le modèle) ----
    let mut cy = H - 150.0;
    let name = if r.billing_name.is_empty() { r.partner_name.clone() } else { r.billing_name.clone() };
    for l in name.lines() {
        c.text(col2, cy, 10.0, false, l);
        cy -= 13.0;
    }
    for l in r.billing_address.lines() {
        c.text(col2, cy, 10.0, false, l);
        cy -= 13.0;
    }
    if !r.billing_address.contains("France") && !r.billing_address.is_empty() && !is_draft {
        c.text(col2, cy, 10.0, false, "France");
        cy -= 13.0;
    }
    cy -= 10.0;
    if !r.billing_vat_number.is_empty() {
        c.text(col2, cy, 10.0, false, &format!("TVA: {}", r.billing_vat_number));
        cy -= 13.0;
    }
    if is_draft {
        c.color(light.0, light.1, light.2);
        c.text(col2, cy, 8.5, false, "Adresse figée à la validation");
        cy -= 13.0;
    }

    // ---- Titre ----
    y = cy.min(H - 250.0) - 18.0;
    c.color(light.0, light.1, light.2);
    c.text(M, y, 20.0, true, &title);
    y -= 30.0;
    c.color(dark.0, dark.1, dark.2);
    c.text(M, y, 9.5, true, "Date de facturation");
    c.text(col2, y, 9.5, true, if r.kind == "AVOIR" { "Facture d'origine" } else { "Échéance" });
    y -= 14.0;
    c.text(M, y, 9.5, false, &fr_date(&r.invoice_date));
    let right_val = if r.kind == "AVOIR" { r.reversed_number.clone().unwrap_or_default() } else { fr_date(&r.due_date) };
    c.text(col2, y, 9.5, false, &right_val);
    y -= 30.0;

    // ---- Tableau ----
    let cols: Vec<(&str, f32, bool)> = if with_vat {
        vec![("Description", M + 4.0, false), ("Quantité", 390.0, true), ("Prix unitaire", 460.0, true), ("TVA", 500.0, true), ("Montant", xr - 4.0, true)]
    } else {
        vec![("Description", M + 4.0, false), ("Quantité", 405.0, true), ("Prix unitaire", 480.0, true), ("Montant", xr - 4.0, true)]
    };
    let desc_width = if with_vat { 280.0 } else { 300.0 };
    let header = |c: &mut Canvas, y: f32| {
        c.color(dark.0, dark.1, dark.2);
        for (label, x, right) in &cols {
            if *right { c.text_right(*x, y, 9.5, true, label); } else { c.text(*x, y, 9.5, true, label); }
        }
        c.color(0.6, 0.6, 0.6);
        c.line(M, y - 6.0, xr, y - 6.0, 0.8);
    };
    header(&mut c, y);
    y -= 26.0;
    for l in &d.lines {
        let desc = wrap(&l.description, 9.5, false, desc_width);
        let needed = desc.len() as f32 * 12.0 + 12.0;
        if y - needed < BOTTOM + 30.0 {
            footer(&mut c, cs, &number, page_no);
            c.new_page();
            page_no += 1;
            y = H - M - 10.0;
            header(&mut c, y);
            y -= 26.0;
        }
        c.color(dark.0, dark.1, dark.2);
        let top_line = y;
        for (i, dl) in desc.iter().enumerate() {
            c.text(M + 4.0, top_line - i as f32 * 12.0, 9.5, false, dl);
        }
        let unit = if l.unit.is_empty() { String::new() } else { format!(" {}", unit_label(&l.unit)) };
        let values: Vec<String> = if with_vat {
            vec![format!("{}{unit}", qty2(l.quantity_milli)), money_plain(l.unit_price), format!("{} %", l.vat_rate_bp as f64 / 100.0).replace('.', ","), money(l.total_ht)]
        } else {
            vec![format!("{}{unit}", qty2(l.quantity_milli)), money_plain(l.unit_price), money(l.total_ht)]
        };
        for ((_, x, _), v) in cols.iter().skip(1).zip(values) {
            c.text_right(*x, top_line, 9.5, false, &v);
        }
        y = top_line - desc.len() as f32 * 12.0 - 12.0;
    }

    // ---- Totaux (bloc droit souligné, comme le modèle) ----
    let mut totals: Vec<(String, String, bool)> = Vec::new();
    if with_vat {
        totals.push(("Montant HT".into(), money(r.total_ht), false));
        totals.push(("TVA".into(), money(r.total_vat), false));
    }
    totals.push(("Total".into(), money(r.total_ttc), true));
    if r.kind == "FACTURE" && r.amount_paid > 0 {
        totals.push(("Payé".into(), money(r.amount_paid), false));
        totals.push(("Reste à payer".into(), money(r.amount_due), true));
    }
    let needed = totals.len() as f32 * 16.0 + 20.0;
    if y - needed < BOTTOM {
        footer(&mut c, cs, &number, page_no);
        c.new_page();
        page_no += 1;
        y = H - M - 10.0;
    }
    let tx = 420.0;
    c.color(0.6, 0.6, 0.6);
    c.line(tx, y + 4.0, xr, y + 4.0, 0.8);
    y -= 12.0;
    let totals_top = y;
    for (k, v, strong) in &totals {
        if *strong { c.color(light.0, light.1, light.2); } else { c.color(grey.0, grey.1, grey.2); }
        c.text_right(xr - 70.0, y, 9.5, *strong, k);
        c.text_right(xr - 4.0, y, 9.5, *strong, v);
        y -= 15.0;
    }

    // ---- Conditions et paiement (gauche, à hauteur des totaux) ----
    let mut ly = totals_top;
    c.color(dark.0, dark.1, dark.2);
    if !r.payment_terms.is_empty() {
        for l in wrap(&format!("Conditions de règlement : {}", r.payment_terms), 9.5, false, 330.0) {
            c.text(M, ly, 9.5, false, &l);
            ly -= 13.0;
        }
    }
    y = y.min(ly) - 18.0;
    if r.kind == "FACTURE" {
        if !is_draft {
            c.text(M, y, 9.5, false, "Communication de paiement : ");
            c.text(M + text_width("Communication de paiement : ", 9.5, false), y, 9.5, true, &number);
            y -= 14.0;
        }
        if !cs.iban.is_empty() {
            let bank = if cs.bank_name.is_empty() { String::new() } else { format!(" - {}", cs.bank_name) };
            c.text(M, y, 9.5, false, "sur ce compte : ");
            c.text(M + text_width("sur ce compte : ", 9.5, false), y, 9.5, true, &format!("{}{bank}", group_iban(&cs.iban)));
            y -= 14.0;
            if !cs.bic.is_empty() {
                c.text(M, y, 9.5, false, &format!("BIC : {}", cs.bic));
                y -= 14.0;
            }
        }
        y -= 8.0;
    }

    // ---- Mentions ----
    let vat_mention = if is_draft && cs.vat_regime == "FRANCHISE" { cs.vat_exemption_mention.clone() } else { r.vat_mention.clone() };
    let legal = if is_draft { cs.legal_mentions.clone() } else { r.legal_mentions.clone() };
    let blocks: Vec<String> = [vat_mention, r.notes.clone(), legal].into_iter().filter(|b| !b.trim().is_empty()).collect();
    for body in blocks {
        let lines = wrap(&body, 8.5, false, W - 2.0 * M);
        let needed = lines.len() as f32 * 11.0 + 6.0;
        if y - needed < BOTTOM {
            footer(&mut c, cs, &number, page_no);
            c.new_page();
            page_no += 1;
            y = H - M - 10.0;
        }
        c.color(grey.0, grey.1, grey.2);
        for l in lines {
            c.text(M, y, 8.5, false, &l);
            y -= 11.0;
        }
        y -= 8.0;
    }
    footer(&mut c, cs, &number, page_no);
    c.new_page();
    build_pdf(c.pages, image.as_ref(), &title, attachment)
}

// Libellé d'unité imprimé ("h" -> "Heures").
fn unit_label(u: &str) -> String {
    match u.to_lowercase().as_str() {
        "h" | "heure" | "heures" => "Heure".into(),
        "j" | "jour" | "jours" => "Jour".into(),
        "km" => "km".into(),
        "forfait" => "Forfait".into(),
        other => other.to_string(),
    }
}

/// Quantité avec deux décimales ("35,00"), comme le modèle Odoo.
// Quantité en millièmes -> "7,00".
fn qty2(milli: i64) -> String {
    let s = format!("{:.2}", milli as f64 / 1000.0);
    s.replace('.', ",")
}

/// Montant sans symbole ("50,00") pour le prix unitaire.
// Montant sans symbole ("1 234,56").
fn money_plain(c: i64) -> String {
    Money(c).format_fr().trim_end_matches('€').trim_end_matches('\u{a0}').to_string()
}

// IBAN groupé par 4 pour la lecture.
fn group_iban(iban: &str) -> String {
    iban.chars().filter(|c| !c.is_whitespace()).collect::<Vec<_>>().chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

// Pied de page : coordonnées, SIRET, NDA, numéro et "Page x / @@PAGES@@" (remplacé à l'assemblage).
fn footer(c: &mut Canvas, cs: &CompanySettings, number: &str, page: usize) {
    c.color(0.6, 0.6, 0.6);
    c.line(M, 92.0, W - M, 92.0, 0.6);
    c.color(0.13, 0.13, 0.13);
    let center = |c: &mut Canvas, y: f32, size: f32, bold: bool, s: &str| {
        let w = text_width(s, size, bold);
        c.text((W - w) / 2.0, y, size, bold, s);
    };
    let contact: Vec<String> = [cs.phone.clone(), cs.email.clone(), cs.website.clone()].into_iter().filter(|s| !s.is_empty()).collect();
    let mut y = 76.0;
    if !contact.is_empty() {
        center(c, y, 9.0, false, &contact.join(" "));
        y -= 13.0;
    }
    let id = if !cs.siret.is_empty() { format!("SIRET : {}", cs.siret) } else if !cs.siren.is_empty() { format!("SIREN : {}", cs.siren) } else { String::new() };
    if !id.is_empty() {
        center(c, y, 9.0, false, &id);
        y -= 13.0;
    }
    if !cs.nda.is_empty() {
        center(c, y, 9.0, true, &format!("NDA : {}", cs.nda));
        y -= 13.0;
    }
    center(c, y, 9.0, false, number);
    y -= 13.0;
    c.color(0.45, 0.45, 0.45);
    center(c, y, 8.5, false, &format!("Page {page} / @@PAGES@@"));
}

// Assemble les objets PDF (catalogue, pages, polices, image, contenus) et la table xref.
fn build_pdf(pages: Vec<Vec<u8>>, image: Option<&Image>, title: &str, attachment: Option<Attachment<'_>>) -> AppResult<Vec<u8>> {
    let n_pages = pages.len();
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // Numérotation : 1 catalogue, 2 pages, 3 police, 4 police grasse, 5 infos,
    // [6 fichier embarqué, 7 descripteur, 8 métadonnées XMP] si Factur-X, [image, masque] si logo, puis pages.
    let mut next = 6;
    let (emb_id, spec_id, xmp_id) = if attachment.is_some() {
        next += 3;
        (next - 3, next - 2, next - 1)
    } else {
        (0, 0, 0)
    };
    let (img_id, mask_id) = if image.is_some() {
        next += 2;
        (next - 2, next - 1)
    } else {
        (0, 0)
    };
    let first_page = next;
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", first_page + i * 2)).collect();
    // Catalogue : avec Factur-X, le fichier est déclaré dans /Names et /AF, et les métadonnées XMP dans /Metadata.
    let catalog = if attachment.is_some() {
        format!(
            "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [(factur-x.xml) {spec_id} 0 R] >> >> /AF [{spec_id} 0 R] /Metadata {xmp_id} 0 R /MarkInfo << /Marked true >> >>"
        )
    } else {
        "<< /Type /Catalog /Pages 2 0 R >>".to_string()
    };
    objects.push(catalog.into_bytes());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n_pages} >>", kids.join(" ")).into_bytes());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
    let mut info = b"<< /Producer (Mercure) /Title ".to_vec();
    info.extend(pdf_string(title));
    info.extend(b" >>");
    objects.push(info);
    if let Some(att) = &attachment {
        // Flux du fichier XML (non compressé pour rester lisible par tout lecteur), descripteur et XMP.
        let mut emb = format!(
            "<< /Type /EmbeddedFile /Subtype /text#2Fxml /Params << /Size {} >> /Length {} >>\nstream\n",
            att.xml.len(), att.xml.len()
        )
        .into_bytes();
        emb.extend(att.xml);
        emb.extend(b"\nendstream");
        objects.push(emb);
        objects.push(format!(
            "<< /Type /Filespec /F (factur-x.xml) /UF (factur-x.xml) /Desc (Factur-X invoice) /AFRelationship /Alternative /EF << /F {emb_id} 0 R /UF {emb_id} 0 R >> >>"
        ).into_bytes());
        let xmp = att.xmp.as_bytes();
        let mut meta = format!("<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n", xmp.len()).into_bytes();
        meta.extend(xmp);
        meta.extend(b"\nendstream");
        objects.push(meta);
    }
    if let Some(img) = image {
        let mut o = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8 /Filter {} /Length {}{} >>\nstream\n",
            img.width, img.height, img.color_space, img.filter, img.data.len(),
            if img.smask.is_some() { format!(" /SMask {mask_id} 0 R") } else { String::new() }
        )
        .into_bytes();
        o.extend(&img.data);
        o.extend(b"\nendstream");
        objects.push(o);
        let mask = img.smask.clone().unwrap_or_else(|| zlib(&vec![255u8; (img.width * img.height) as usize]));
        let mut m = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
            img.width, img.height, mask.len()
        )
        .into_bytes();
        m.extend(&mask);
        m.extend(b"\nendstream");
        objects.push(m);
    }
    let xobj = if image.is_some() { format!(" /XObject << /Im1 {img_id} 0 R >>") } else { String::new() };
    let marker = b"@@PAGES@@";
    let total = n_pages.to_string();
    for (i, content) in pages.into_iter().enumerate() {
        let page_id = first_page + i * 2;
        // Remplace le marqueur du nombre total de pages (sur les octets bruts WinAnsi).
        let mut raw = Vec::with_capacity(content.len());
        let mut k = 0;
        while k < content.len() {
            if content[k..].starts_with(marker) {
                raw.extend_from_slice(total.as_bytes());
                k += marker.len();
            } else {
                raw.push(content[k]);
                k += 1;
            }
        }
        let packed = zlib(&raw);
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >>{xobj} >> /Contents {} 0 R >>",
                page_id + 1
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        stream.extend(packed);
        stream.extend(b"\nendstream");
        objects.push(stream);
    }
    if objects.len() != first_page - 1 + 2 * n_pages {
        return Err(AppError::internal("numérotation des objets PDF incohérente"));
    }
    // Assemblage avec table des références croisées (PDF 1.7 requis pour les pièces jointes /AF).
    let mut out: Vec<u8> = if attachment.is_some() { b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec() } else { b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec() };
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodage_et_largeurs() {
        assert_eq!(pdf_string("Été (1 €)"), vec![b'(', 0xC9, b't', 0xE9, b' ', b'\\', b'(', b'1', b' ', 0x80, b'\\', b')', b')']);
        assert!((text_width("é", 10.0, false) - text_width("e", 10.0, false)).abs() < 0.001);
        assert!(text_width("WWW", 10.0, true) > text_width("iii", 10.0, true));
    }

    #[test]
    fn retour_a_la_ligne() {
        let lines = wrap("Formation AutoCAD niveau avancé pour une équipe de dessinateurs", 10.0, false, 120.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| text_width(l, 10.0, false) <= 120.0));
        assert_eq!(wrap("a\nb", 10.0, false, 100.0), vec!["a", "b"]);
    }

    #[test]
    fn quantites_et_iban() {
        assert_eq!(qty2(14_000), "14,00");
        assert_eq!(qty2(1_500), "1,50");
        assert_eq!(money_plain(5000), "50,00");
        assert_eq!(group_iban("FR7616958000013704550170433"), "FR76 1695 8000 0137 0455 0170 433");
    }
}
