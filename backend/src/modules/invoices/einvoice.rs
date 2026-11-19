//! Facture électronique : génération des formats structurés du socle français.
//!
//! - CII (UN/CEFACT Cross Industry Invoice), profil EN 16931 : c'est le XML embarqué dans un PDF Factur-X ;
//! - UBL 2.1 (Invoice / CreditNote).
//!
//! Le fichier produit est destiné à être déposé sur la plateforme agréée (PA) choisie par l'entreprise,
//! qui le contrôle et l'achemine au client ; Mercure ne transmet rien lui-même.
//! Les montants sont écrits en euros avec deux décimales à partir des centimes stockés (jamais de flottant).

use crate::modules::settings::CompanySettings;

use super::service::{InvoiceDetail, Line};

/// Centimes -> "1234.56".
fn amt(c: i64) -> String {
    let neg = c < 0;
    let a = c.abs();
    format!("{}{}.{:02}", if neg { "-" } else { "" }, a / 100, a % 100)
}

/// Millièmes -> "7.000" (quantités) ; points de base -> "20.00" (taux).
fn qty(m: i64) -> String {
    format!("{}.{:03}", m / 1000, (m % 1000).abs())
}
fn rate(bp: i64) -> String {
    format!("{}.{:02}", bp / 100, (bp % 100).abs())
}

/// Échappement XML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// "2026-11-18" -> "20261118" (format 102 du CII).
fn date102(d: &str) -> String {
    d.replace('-', "")
}

/// Code pays ISO 3166-1 alpha-2 d'après le libellé saisi (France par défaut).
pub fn country_code(label: &str) -> &'static str {
    let l = label.trim().to_lowercase();
    match l.as_str() {
        "" | "france" | "fr" => "FR",
        "belgique" | "belgium" | "be" => "BE",
        "suisse" | "switzerland" | "ch" => "CH",
        "luxembourg" | "lu" => "LU",
        "allemagne" | "germany" | "de" => "DE",
        "espagne" | "spain" | "es" => "ES",
        "italie" | "italy" | "it" => "IT",
        "royaume-uni" | "united kingdom" | "uk" | "gb" => "GB",
        "monaco" | "mc" => "MC",
        "canada" | "ca" => "CA",
        _ => "FR",
    }
}

/// Code unité UN/ECE Recommandation 20 d'après l'unité saisie.
fn unit_code(u: &str) -> &'static str {
    match u.trim().to_lowercase().as_str() {
        "h" | "heure" | "heures" => "HUR",
        "j" | "jour" | "jours" => "DAY",
        "km" => "KMT",
        "forfait" | "ff" => "LS",
        _ => "C62", // unité (pièce)
    }
}

/// Adresse de facturation figée (lignes) -> (rue, complément, code postal, ville, pays).
/// Le format figé est : lignes de rue, puis "CP Ville", puis éventuellement le pays.
fn split_address(block: &str) -> (String, String, String, String, String) {
    let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut street = String::new();
    let mut street2 = String::new();
    let mut zip = String::new();
    let mut city = String::new();
    let mut country = String::new();
    let is_cp_city = |l: &str| l.len() > 6 && l.chars().take(5).all(|c| c.is_ascii_digit()) && l.chars().nth(5) == Some(' ');
    let mut i = 0;
    while i < lines.len() && !is_cp_city(lines[i]) {
        if street.is_empty() {
            street = lines[i].to_string();
        } else if street2.is_empty() {
            street2 = lines[i].to_string();
        } else {
            street2 = format!("{street2} {}", lines[i]);
        }
        i += 1;
    }
    if i < lines.len() {
        zip = lines[i][..5].to_string();
        city = lines[i][6..].trim().to_string();
        if i + 1 < lines.len() {
            country = lines[i + 1].to_string();
        }
    }
    (street, street2, zip, city, country)
}

/// Code de catégorie de TVA EN 16931 : S (taux standard) ou E (exonéré / franchise), avec motif.
fn vat_category(d: &InvoiceDetail, cs: &CompanySettings, rate_bp: i64) -> (&'static str, Option<String>, Option<&'static str>) {
    if rate_bp > 0 {
        ("S", None, None)
    } else if cs.vat_regime == "FRANCHISE" {
        // Franchise en base : code VATEX-FR-FRANCHISE de l'extension française.
        (
            "E",
            Some(if d.row.vat_mention.is_empty() { cs.vat_exemption_mention.clone() } else { d.row.vat_mention.clone() }),
            Some("VATEX-FR-FRANCHISE"),
        )
    } else {
        ("E", Some("Exonération de TVA".to_string()), Some("VATEX-EU-132"))
    }
}

/// Lignes regroupées par taux de TVA : (taux bp, base HT, montant TVA).
fn vat_breakdown(d: &InvoiceDetail) -> Vec<(i64, i64, i64)> {
    let mut out: Vec<(i64, i64, i64)> = Vec::new();
    for l in &d.lines {
        match out.iter_mut().find(|(r, _, _)| *r == l.vat_rate_bp) {
            Some(e) => {
                e.1 += l.total_ht;
                e.2 += l.total_vat;
            }
            None => out.push((l.vat_rate_bp, l.total_ht, l.total_vat)),
        }
    }
    out.sort_by_key(|e| e.0);
    out
}

/// Code du type de document : 380 facture, 381 avoir.
fn type_code(d: &InvoiceDetail) -> &'static str {
    if d.row.kind == "AVOIR" { "381" } else { "380" }
}

/// Code BT-23 (processus métier) de la réforme française selon la catégorie d'opération.
/// A1 livraison de biens, A2 prestation de services, A3 mixte (spécifications externes DGFiP).
fn business_process(cat: &str) -> &'static str {
    match cat {
        "BIENS" => "A1",
        "MIXTE" => "A3",
        _ => "A2",
    }
}

/// Parties et montants communs aux deux formats.
struct Parties {
    seller_name: String,
    seller_siren: String,
    seller_vat: String,
    seller_street: String,
    seller_street2: String,
    seller_zip: String,
    seller_city: String,
    seller_country: &'static str,
    buyer_name: String,
    buyer_siren: String,
    buyer_vat: String,
    buyer_street: String,
    buyer_street2: String,
    buyer_zip: String,
    buyer_city: String,
    buyer_country: &'static str,
}

fn parties(d: &InvoiceDetail, cs: &CompanySettings) -> Parties {
    let (bs, bs2, bz, bc, bco) = split_address(&d.row.billing_address);
    let seller_siren = if cs.siren.is_empty() { cs.siret.chars().take(9).collect() } else { cs.siren.clone() };
    Parties {
        seller_name: cs.legal_name.clone(),
        seller_siren,
        seller_vat: cs.vat_number.clone(),
        seller_street: cs.address_line1.clone(),
        seller_street2: cs.address_line2.clone(),
        seller_zip: cs.zip.clone(),
        seller_city: cs.city.clone(),
        seller_country: country_code(&cs.country),
        // Le nom figé peut contenir "Société\nPersonne" : la première ligne est la raison sociale.
        buyer_name: d.row.billing_name.lines().next().unwrap_or("").to_string(),
        buyer_siren: d.row.billing_siren.clone(),
        buyer_vat: d.row.billing_vat_number.clone(),
        buyer_street: bs,
        buyer_street2: bs2,
        buyer_zip: bz,
        buyer_city: bc,
        buyer_country: country_code(&bco),
    }
}

/// Le document est-il émis (numéroté) ? Les brouillons n'ont pas de représentation électronique valable.
pub fn ensure_issued(d: &InvoiceDetail) -> crate::error::AppResult<()> {
    if d.row.number.is_none() {
        return Err(crate::error::AppError::Validation("Validez la facture avant de générer sa version électronique".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CII (Factur-X, profil EN 16931)
// ---------------------------------------------------------------------------

/// XML CII complet (encodage UTF-8), conforme au profil EN 16931 de Factur-X 1.0.
pub fn cii(d: &InvoiceDetail, cs: &CompanySettings) -> String {
    let p = parties(d, cs);
    let r = &d.row;
    let number = r.number.clone().unwrap_or_default();
    let mut x = String::with_capacity(8192);
    x.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
  xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100"
  xmlns:qdt="urn:un:unece:uncefact:data:standard:QualifiedDataType:100"
  xmlns:udt="urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100">
"#);
    // Contexte : processus métier (catégorie d'opération) et profil EN 16931.
    x.push_str(&format!(
        "  <rsm:ExchangedDocumentContext>\n    <ram:BusinessProcessSpecifiedDocumentContextParameter><ram:ID>{}</ram:ID></ram:BusinessProcessSpecifiedDocumentContextParameter>\n    <ram:GuidelineSpecifiedDocumentContextParameter><ram:ID>urn:cen.eu:en16931:2017</ram:ID></ram:GuidelineSpecifiedDocumentContextParameter>\n  </rsm:ExchangedDocumentContext>\n",
        business_process(&r.operation_category)
    ));
    // En-tête : numéro, type, date, notes.
    x.push_str(&format!(
        "  <rsm:ExchangedDocument>\n    <ram:ID>{}</ram:ID>\n    <ram:TypeCode>{}</ram:TypeCode>\n    <ram:IssueDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:IssueDateTime>\n",
        esc(&number), type_code(d), date102(&r.invoice_date)
    ));
    for note in [r.notes.as_str(), r.vat_mention.as_str(), r.legal_mentions.as_str()].iter().filter(|n| !n.trim().is_empty()) {
        x.push_str(&format!("    <ram:IncludedNote><ram:Content>{}</ram:Content></ram:IncludedNote>\n", esc(note.trim())));
    }
    if cs.vat_on_debits {
        x.push_str("    <ram:IncludedNote><ram:Content>Option pour le paiement de la TVA d'après les débits</ram:Content></ram:IncludedNote>\n");
    }
    x.push_str("  </rsm:ExchangedDocument>\n  <rsm:SupplyChainTradeTransaction>\n");
    // Lignes.
    for (i, l) in d.lines.iter().enumerate() {
        let (cat, _, _) = vat_category(d, cs, l.vat_rate_bp);
        x.push_str(&format!(
            "    <ram:IncludedSupplyChainTradeLineItem>\n      <ram:AssociatedDocumentLineDocument><ram:LineID>{}</ram:LineID></ram:AssociatedDocumentLineDocument>\n      <ram:SpecifiedTradeProduct><ram:Name>{}</ram:Name></ram:SpecifiedTradeProduct>\n      <ram:SpecifiedLineTradeAgreement><ram:NetPriceProductTradePrice><ram:ChargeAmount>{}</ram:ChargeAmount></ram:NetPriceProductTradePrice></ram:SpecifiedLineTradeAgreement>\n      <ram:SpecifiedLineTradeDelivery><ram:BilledQuantity unitCode=\"{}\">{}</ram:BilledQuantity></ram:SpecifiedLineTradeDelivery>\n      <ram:SpecifiedLineTradeSettlement>\n        <ram:ApplicableTradeTax><ram:TypeCode>VAT</ram:TypeCode><ram:CategoryCode>{}</ram:CategoryCode><ram:RateApplicablePercent>{}</ram:RateApplicablePercent></ram:ApplicableTradeTax>\n        <ram:SpecifiedTradeSettlementLineMonetarySummation><ram:LineTotalAmount>{}</ram:LineTotalAmount></ram:SpecifiedTradeSettlementLineMonetarySummation>\n      </ram:SpecifiedLineTradeSettlement>\n    </ram:IncludedSupplyChainTradeLineItem>\n",
            i + 1, esc(line_name(l)), amt(l.unit_price), unit_code(&l.unit), qty(l.quantity_milli), cat, rate(l.vat_rate_bp), amt(l.total_ht)
        ));
    }
    // Vendeur et acheteur.
    x.push_str("    <ram:ApplicableHeaderTradeAgreement>\n");
    x.push_str(&party_cii("SellerTradeParty", &p.seller_name, &p.seller_siren, &p.seller_vat, &p.seller_street, &p.seller_street2, &p.seller_zip, &p.seller_city, p.seller_country, &cs.email));
    x.push_str(&party_cii("BuyerTradeParty", &p.buyer_name, &p.buyer_siren, &p.buyer_vat, &p.buyer_street, &p.buyer_street2, &p.buyer_zip, &p.buyer_city, p.buyer_country, ""));
    x.push_str("    </ram:ApplicableHeaderTradeAgreement>\n    <ram:ApplicableHeaderTradeDelivery/>\n");
    // Règlement : devise, moyen de paiement, ventilation de TVA, échéance, totaux.
    x.push_str("    <ram:ApplicableHeaderTradeSettlement>\n      <ram:InvoiceCurrencyCode>EUR</ram:InvoiceCurrencyCode>\n");
    if !cs.iban.is_empty() {
        x.push_str(&format!(
            "      <ram:SpecifiedTradeSettlementPaymentMeans><ram:TypeCode>30</ram:TypeCode><ram:PayeePartyCreditorFinancialAccount><ram:IBANID>{}</ram:IBANID></ram:PayeePartyCreditorFinancialAccount></ram:SpecifiedTradeSettlementPaymentMeans>\n",
            esc(&cs.iban.replace(' ', ""))
        ));
    }
    for (rate_bp, base, tax) in vat_breakdown(d) {
        let (cat, reason, code) = vat_category(d, cs, rate_bp);
        x.push_str("      <ram:ApplicableTradeTax>\n");
        x.push_str(&format!("        <ram:CalculatedAmount>{}</ram:CalculatedAmount>\n        <ram:TypeCode>VAT</ram:TypeCode>\n", amt(tax)));
        if let Some(rs) = reason {
            x.push_str(&format!("        <ram:ExemptionReason>{}</ram:ExemptionReason>\n", esc(&rs)));
        }
        x.push_str(&format!("        <ram:BasisAmount>{}</ram:BasisAmount>\n        <ram:CategoryCode>{}</ram:CategoryCode>\n", amt(base), cat));
        if let Some(c) = code {
            x.push_str(&format!("        <ram:ExemptionReasonCode>{c}</ram:ExemptionReasonCode>\n"));
        }
        x.push_str(&format!("        <ram:RateApplicablePercent>{}</ram:RateApplicablePercent>\n      </ram:ApplicableTradeTax>\n", rate(rate_bp)));
    }
    x.push_str(&format!(
        "      <ram:SpecifiedTradePaymentTerms>{}<ram:DueDateDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:DueDateDateTime></ram:SpecifiedTradePaymentTerms>\n",
        if r.payment_terms.trim().is_empty() { String::new() } else { format!("<ram:Description>{}</ram:Description>", esc(r.payment_terms.trim())) },
        date102(&r.due_date)
    ));
    x.push_str(&format!(
        "      <ram:SpecifiedTradeSettlementHeaderMonetarySummation>\n        <ram:LineTotalAmount>{ht}</ram:LineTotalAmount>\n        <ram:TaxBasisTotalAmount>{ht}</ram:TaxBasisTotalAmount>\n        <ram:TaxTotalAmount currencyID=\"EUR\">{vat}</ram:TaxTotalAmount>\n        <ram:GrandTotalAmount>{ttc}</ram:GrandTotalAmount>\n        <ram:TotalPrepaidAmount>{paid}</ram:TotalPrepaidAmount>\n        <ram:DuePayableAmount>{due}</ram:DuePayableAmount>\n      </ram:SpecifiedTradeSettlementHeaderMonetarySummation>\n",
        ht = amt(r.total_ht), vat = amt(r.total_vat), ttc = amt(r.total_ttc), paid = amt(r.amount_paid), due = amt(r.total_ttc - r.amount_paid)
    ));
    // Avoir : référence à la facture d'origine.
    if let Some(orig) = &r.reversed_number {
        x.push_str(&format!("      <ram:InvoiceReferencedDocument><ram:IssuerAssignedID>{}</ram:IssuerAssignedID></ram:InvoiceReferencedDocument>\n", esc(orig)));
    }
    x.push_str("    </ram:ApplicableHeaderTradeSettlement>\n  </rsm:SupplyChainTradeTransaction>\n</rsm:CrossIndustryInvoice>\n");
    x
}

/// Libellé de ligne non vide (le CII exige un nom de produit).
fn line_name(l: &Line) -> &str {
    let t = l.description.trim();
    if t.is_empty() { "Prestation" } else { t }
}

#[allow(clippy::too_many_arguments)]
fn party_cii(tag: &str, name: &str, siren: &str, vat: &str, street: &str, street2: &str, zip: &str, city: &str, country: &str, email: &str) -> String {
    let mut x = format!("      <ram:{tag}>\n        <ram:Name>{}</ram:Name>\n", esc(name));
    if !siren.is_empty() {
        // Schéma 0002 = SIREN (ISO 6523).
        x.push_str(&format!("        <ram:SpecifiedLegalOrganization><ram:ID schemeID=\"0002\">{}</ram:ID></ram:SpecifiedLegalOrganization>\n", esc(siren)));
    }
    x.push_str("        <ram:PostalTradeAddress>\n");
    if !zip.is_empty() {
        x.push_str(&format!("          <ram:PostcodeCode>{}</ram:PostcodeCode>\n", esc(zip)));
    }
    if !street.is_empty() {
        x.push_str(&format!("          <ram:LineOne>{}</ram:LineOne>\n", esc(street)));
    }
    if !street2.is_empty() {
        x.push_str(&format!("          <ram:LineTwo>{}</ram:LineTwo>\n", esc(street2)));
    }
    if !city.is_empty() {
        x.push_str(&format!("          <ram:CityName>{}</ram:CityName>\n", esc(city)));
    }
    x.push_str(&format!("          <ram:CountryID>{country}</ram:CountryID>\n        </ram:PostalTradeAddress>\n"));
    if !email.is_empty() {
        x.push_str(&format!("        <ram:URIUniversalCommunication><ram:URIID schemeID=\"EM\">{}</ram:URIID></ram:URIUniversalCommunication>\n", esc(email)));
    }
    if !vat.is_empty() {
        x.push_str(&format!("        <ram:SpecifiedTaxRegistration><ram:ID schemeID=\"VA\">{}</ram:ID></ram:SpecifiedTaxRegistration>\n", esc(&vat.replace(' ', ""))));
    }
    x.push_str(&format!("      </ram:{tag}>\n"));
    x
}

// ---------------------------------------------------------------------------
// UBL 2.1
// ---------------------------------------------------------------------------

/// XML UBL 2.1 (Invoice ou CreditNote) conforme EN 16931.
pub fn ubl(d: &InvoiceDetail, cs: &CompanySettings) -> String {
    let p = parties(d, cs);
    let r = &d.row;
    let credit = r.kind == "AVOIR";
    let root = if credit { "CreditNote" } else { "Invoice" };
    let ns = if credit { "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2" } else { "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2" };
    let qty_tag = if credit { "CreditedQuantity" } else { "InvoicedQuantity" };
    let line_tag = if credit { "CreditNoteLine" } else { "InvoiceLine" };
    let type_tag = if credit { "CreditNoteTypeCode" } else { "InvoiceTypeCode" };
    let number = r.number.clone().unwrap_or_default();
    let mut x = String::with_capacity(8192);
    x.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{root} xmlns=\"{ns}\" xmlns:cac=\"urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2\" xmlns:cbc=\"urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2\">\n"
    ));
    x.push_str(&format!(
        "  <cbc:CustomizationID>urn:cen.eu:en16931:2017</cbc:CustomizationID>\n  <cbc:ProfileID>{}</cbc:ProfileID>\n  <cbc:ID>{}</cbc:ID>\n  <cbc:IssueDate>{}</cbc:IssueDate>\n",
        business_process(&r.operation_category), esc(&number), r.invoice_date
    ));
    if !credit {
        x.push_str(&format!("  <cbc:DueDate>{}</cbc:DueDate>\n", r.due_date));
    }
    x.push_str(&format!("  <cbc:{type_tag}>{}</cbc:{type_tag}>\n", type_code(d)));
    for note in [r.notes.as_str(), r.vat_mention.as_str(), r.legal_mentions.as_str()].iter().filter(|n| !n.trim().is_empty()) {
        x.push_str(&format!("  <cbc:Note>{}</cbc:Note>\n", esc(note.trim())));
    }
    x.push_str("  <cbc:DocumentCurrencyCode>EUR</cbc:DocumentCurrencyCode>\n");
    if let Some(orig) = &r.reversed_number {
        x.push_str(&format!("  <cac:BillingReference><cac:InvoiceDocumentReference><cbc:ID>{}</cbc:ID></cac:InvoiceDocumentReference></cac:BillingReference>\n", esc(orig)));
    }
    x.push_str(&party_ubl("AccountingSupplierParty", &p.seller_name, &p.seller_siren, &p.seller_vat, &p.seller_street, &p.seller_street2, &p.seller_zip, &p.seller_city, p.seller_country, &cs.email));
    x.push_str(&party_ubl("AccountingCustomerParty", &p.buyer_name, &p.buyer_siren, &p.buyer_vat, &p.buyer_street, &p.buyer_street2, &p.buyer_zip, &p.buyer_city, p.buyer_country, ""));
    if !cs.iban.is_empty() {
        x.push_str(&format!(
            "  <cac:PaymentMeans>\n    <cbc:PaymentMeansCode>30</cbc:PaymentMeansCode>\n    <cac:PayeeFinancialAccount><cbc:ID>{}</cbc:ID>{}</cac:PayeeFinancialAccount>\n  </cac:PaymentMeans>\n",
            esc(&cs.iban.replace(' ', "")),
            if cs.bic.is_empty() { String::new() } else { format!("<cac:FinancialInstitutionBranch><cbc:ID>{}</cbc:ID></cac:FinancialInstitutionBranch>", esc(&cs.bic)) }
        ));
    }
    if !r.payment_terms.trim().is_empty() {
        x.push_str(&format!("  <cac:PaymentTerms><cbc:Note>{}</cbc:Note></cac:PaymentTerms>\n", esc(r.payment_terms.trim())));
    }
    // Ventilation de TVA.
    x.push_str(&format!("  <cac:TaxTotal>\n    <cbc:TaxAmount currencyID=\"EUR\">{}</cbc:TaxAmount>\n", amt(r.total_vat)));
    for (rate_bp, base, tax) in vat_breakdown(d) {
        let (cat, reason, code) = vat_category(d, cs, rate_bp);
        x.push_str(&format!(
            "    <cac:TaxSubtotal>\n      <cbc:TaxableAmount currencyID=\"EUR\">{}</cbc:TaxableAmount>\n      <cbc:TaxAmount currencyID=\"EUR\">{}</cbc:TaxAmount>\n      <cac:TaxCategory>\n        <cbc:ID>{}</cbc:ID>\n        <cbc:Percent>{}</cbc:Percent>\n",
            amt(base), amt(tax), cat, rate(rate_bp)
        ));
        if let Some(c) = code {
            x.push_str(&format!("        <cbc:TaxExemptionReasonCode>{c}</cbc:TaxExemptionReasonCode>\n"));
        }
        if let Some(rs) = reason {
            x.push_str(&format!("        <cbc:TaxExemptionReason>{}</cbc:TaxExemptionReason>\n", esc(&rs)));
        }
        x.push_str("        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>\n      </cac:TaxCategory>\n    </cac:TaxSubtotal>\n");
    }
    x.push_str("  </cac:TaxTotal>\n");
    x.push_str(&format!(
        "  <cac:LegalMonetaryTotal>\n    <cbc:LineExtensionAmount currencyID=\"EUR\">{ht}</cbc:LineExtensionAmount>\n    <cbc:TaxExclusiveAmount currencyID=\"EUR\">{ht}</cbc:TaxExclusiveAmount>\n    <cbc:TaxInclusiveAmount currencyID=\"EUR\">{ttc}</cbc:TaxInclusiveAmount>\n    <cbc:PrepaidAmount currencyID=\"EUR\">{paid}</cbc:PrepaidAmount>\n    <cbc:PayableAmount currencyID=\"EUR\">{due}</cbc:PayableAmount>\n  </cac:LegalMonetaryTotal>\n",
        ht = amt(r.total_ht), ttc = amt(r.total_ttc), paid = amt(r.amount_paid), due = amt(r.total_ttc - r.amount_paid)
    ));
    for (i, l) in d.lines.iter().enumerate() {
        let (cat, _, _) = vat_category(d, cs, l.vat_rate_bp);
        x.push_str(&format!(
            "  <cac:{line_tag}>\n    <cbc:ID>{}</cbc:ID>\n    <cbc:{qty_tag} unitCode=\"{}\">{}</cbc:{qty_tag}>\n    <cbc:LineExtensionAmount currencyID=\"EUR\">{}</cbc:LineExtensionAmount>\n    <cac:Item>\n      <cbc:Name>{}</cbc:Name>\n      <cac:ClassifiedTaxCategory><cbc:ID>{}</cbc:ID><cbc:Percent>{}</cbc:Percent><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:ClassifiedTaxCategory>\n    </cac:Item>\n    <cac:Price><cbc:PriceAmount currencyID=\"EUR\">{}</cbc:PriceAmount></cac:Price>\n  </cac:{line_tag}>\n",
            i + 1, unit_code(&l.unit), qty(l.quantity_milli), amt(l.total_ht), esc(line_name(l)), cat, rate(l.vat_rate_bp), amt(l.unit_price)
        ));
    }
    x.push_str(&format!("</{root}>\n"));
    x
}

#[allow(clippy::too_many_arguments)]
fn party_ubl(tag: &str, name: &str, siren: &str, vat: &str, street: &str, street2: &str, zip: &str, city: &str, country: &str, email: &str) -> String {
    let mut x = format!("  <cac:{tag}>\n    <cac:Party>\n");
    if !siren.is_empty() {
        x.push_str(&format!("      <cac:PartyIdentification><cbc:ID schemeID=\"0002\">{}</cbc:ID></cac:PartyIdentification>\n", esc(siren)));
    }
    x.push_str(&format!("      <cac:PartyName><cbc:Name>{}</cbc:Name></cac:PartyName>\n      <cac:PostalAddress>\n", esc(name)));
    if !street.is_empty() {
        x.push_str(&format!("        <cbc:StreetName>{}</cbc:StreetName>\n", esc(street)));
    }
    if !street2.is_empty() {
        x.push_str(&format!("        <cbc:AdditionalStreetName>{}</cbc:AdditionalStreetName>\n", esc(street2)));
    }
    if !city.is_empty() {
        x.push_str(&format!("        <cbc:CityName>{}</cbc:CityName>\n", esc(city)));
    }
    if !zip.is_empty() {
        x.push_str(&format!("        <cbc:PostalZone>{}</cbc:PostalZone>\n", esc(zip)));
    }
    x.push_str(&format!("        <cac:Country><cbc:IdentificationCode>{country}</cbc:IdentificationCode></cac:Country>\n      </cac:PostalAddress>\n"));
    if !vat.is_empty() {
        x.push_str(&format!("      <cac:PartyTaxScheme><cbc:CompanyID>{}</cbc:CompanyID><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>\n", esc(&vat.replace(' ', ""))));
    }
    x.push_str(&format!("      <cac:PartyLegalEntity><cbc:RegistrationName>{}</cbc:RegistrationName>{}</cac:PartyLegalEntity>\n",
        esc(name),
        if siren.is_empty() { String::new() } else { format!("<cbc:CompanyID schemeID=\"0002\">{}</cbc:CompanyID>", esc(siren)) }));
    if !email.is_empty() {
        x.push_str(&format!("      <cac:Contact><cbc:ElectronicMail>{}</cbc:ElectronicMail></cac:Contact>\n", esc(email)));
    }
    x.push_str(&format!("    </cac:Party>\n  </cac:{tag}>\n"));
    x
}

// ---------------------------------------------------------------------------
// Métadonnées XMP du PDF Factur-X
// ---------------------------------------------------------------------------

/// Paquet XMP déclarant PDF/A-3 et l'extension Factur-X (profil EN 16931).
pub fn xmp(_d: &InvoiceDetail, title: &str) -> String {
    // Factur-X : un avoir reste de type INVOICE dans le XMP (le TypeCode 381 est porté par le XML).
    let doc_type = "INVOICE";
    format!(
        r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
      <pdfaid:part>3</pdfaid:part>
      <pdfaid:conformance>B</pdfaid:conformance>
    </rdf:Description>
    <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">
      <dc:title><rdf:Alt><rdf:li xml:lang="x-default">{title}</rdf:li></rdf:Alt></dc:title>
      <dc:creator><rdf:Seq><rdf:li>Mercure</rdf:li></rdf:Seq></dc:creator>
    </rdf:Description>
    <rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/">
      <pdf:Producer>Mercure</pdf:Producer>
    </rdf:Description>
    <rdf:Description rdf:about="" xmlns:pdfaExtension="http://www.aiim.org/pdfa/ns/extension/" xmlns:pdfaSchema="http://www.aiim.org/pdfa/ns/schema#" xmlns:pdfaProperty="http://www.aiim.org/pdfa/ns/property#">
      <pdfaExtension:schemas>
        <rdf:Bag>
          <rdf:li rdf:parseType="Resource">
            <pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema>
            <pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI>
            <pdfaSchema:prefix>fx</pdfaSchema:prefix>
            <pdfaSchema:property>
              <rdf:Seq>
                <rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentFileName</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>name of the embedded XML invoice file</pdfaProperty:description></rdf:li>
                <rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentType</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>INVOICE</pdfaProperty:description></rdf:li>
                <rdf:li rdf:parseType="Resource"><pdfaProperty:name>Version</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The actual version of the Factur-X XML schema</pdfaProperty:description></rdf:li>
                <rdf:li rdf:parseType="Resource"><pdfaProperty:name>ConformanceLevel</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The conformance level of the embedded Factur-X data</pdfaProperty:description></rdf:li>
              </rdf:Seq>
            </pdfaSchema:property>
          </rdf:li>
        </rdf:Bag>
      </pdfaExtension:schemas>
    </rdf:Description>
    <rdf:Description rdf:about="" xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
      <fx:DocumentType>{doc_type}</fx:DocumentType>
      <fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
      <fx:Version>1.0</fx:Version>
      <fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        title = esc(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn montants_et_quantites() {
        assert_eq!(amt(123456), "1234.56");
        assert_eq!(amt(-5), "-0.05");
        assert_eq!(qty(7000), "7.000");
        assert_eq!(qty(1500), "1.500");
        assert_eq!(rate(2000), "20.00");
        assert_eq!(rate(550), "5.50");
        assert_eq!(date102("2026-11-18"), "20261118");
    }

    #[test]
    fn decoupage_adresse() {
        let (s, s2, z, c, p) = split_address("12 rue des Lilas\nBât. B\n33000 Bordeaux\nFrance");
        assert_eq!((s.as_str(), s2.as_str(), z.as_str(), c.as_str(), p.as_str()), ("12 rue des Lilas", "Bât. B", "33000", "Bordeaux", "France"));
        let (s, _, z, c, _) = split_address("33000 Bordeaux");
        assert_eq!((s.as_str(), z.as_str(), c.as_str()), ("", "33000", "Bordeaux"));
        assert_eq!(country_code("France"), "FR");
        assert_eq!(country_code("Belgique"), "BE");
        assert_eq!(unit_code("h"), "HUR");
        assert_eq!(unit_code("jour"), "DAY");
    }
}
