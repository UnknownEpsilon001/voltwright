// Bill of Materials: grouping, counting, CSV / HTML / text export.

use std::collections::BTreeMap;

use crate::model::{Library, Project};

#[derive(Clone, Debug)]
pub struct BomRow {
    pub refdes: Vec<String>,
    pub value: String,
    pub device: String,
    pub package: String,
    pub library: String,
    pub qty: usize,
}

pub fn build(proj: &Project, lib: &Library) -> Vec<BomRow> {
    let mut map: BTreeMap<(String, String, String, String), BomRow> = BTreeMap::new();
    for c in &proj.comps {
        let def = lib.get(&c.part);
        let device = def.map(|d| d.id.clone()).unwrap_or_else(|| c.part.clone());
        let package = def
            .map(|d| d.package.clone())
            .unwrap_or_else(|| "NONE".into());
        let library = def
            .map(|d| d.library.clone())
            .unwrap_or_else(|| String::new());
        // power symbols and frames are not real parts
        if def
            .map(|d| d.prefix == "#PWR" || d.prefix == "FRAME")
            .unwrap_or(false)
        {
            continue;
        }
        let key = (
            c.value.clone(),
            device.clone(),
            package.clone(),
            library.clone(),
        );
        map.entry(key)
            .or_insert_with(|| BomRow {
                refdes: Vec::new(),
                value: c.value.clone(),
                device,
                package,
                library,
                qty: 0,
            })
            .refdes
            .push(c.refdes.clone());
    }
    let mut rows: Vec<BomRow> = map.into_values().collect();
    for r in &mut rows {
        r.refdes.sort();
        r.qty = r.refdes.len();
    }
    rows.sort_by(|a, b| {
        natural_key(&a.refdes.first().cloned().unwrap_or_default())
            .cmp(&natural_key(&b.refdes.first().cloned().unwrap_or_default()))
            .then_with(|| a.value.cmp(&b.value))
    });
    rows
}

fn natural_key(s: &str) -> (String, u32) {
    let split = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let (head, tail) = s.split_at(split);
    let n = tail.parse::<u32>().unwrap_or(0);
    (head.to_string(), n)
}

pub fn total_parts(rows: &[BomRow]) -> usize {
    rows.iter().map(|r| r.qty).sum()
}

pub fn to_csv(rows: &[BomRow]) -> String {
    let mut out = String::from("Qty,References,Value,Device,Package,Library\r\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{}\r\n",
            r.qty,
            r.refdes.join(" "),
            esc(&r.value),
            esc(&r.device),
            esc(&r.package),
            esc(&r.library),
        ));
    }
    out
}

pub fn to_text(proj: &Project, rows: &[BomRow]) -> String {
    let mut out = format!("Bill of Materials - {}\r\n", proj.name);
    out.push_str("======================================================\r\n");
    out.push_str(&format!(
        "{:<6} {:<12} {:<22} {:<14} {}\r\n",
        "Qty", "Refs", "Value", "Device", "Package"
    ));
    out.push_str("------------------------------------------------------\r\n");
    for r in rows {
        out.push_str(&format!(
            "{:<6} {:<12} {:<22} {:<14} {}\r\n",
            r.qty,
            r.refdes.join(" "),
            r.value,
            r.device,
            r.package
        ));
    }
    out.push_str("------------------------------------------------------\r\n");
    out.push_str(&format!(
        "{} line items, {} components\r\n",
        rows.len(),
        total_parts(rows)
    ));
    out
}

pub fn to_html(proj: &Project, rows: &[BomRow]) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\r\n<html><head><meta charset=\"utf-8\">");
    out.push_str("<title>Bill of Materials - ");
    out.push_str(&esc(&proj.name));
    out.push_str("</title><style>");
    out.push_str(
        "body{font-family:Segoe UI,Arial,sans-serif;margin:24px;background:#f4f4f0;color:#202020}",
    );
    out.push_str("h1{font-size:20px;margin:0 0 4px}table{border-collapse:collapse;width:100%;background:#fff}");
    out.push_str("th{background:#e0e0d0;text-align:left;padding:6px 10px;border:1px solid #b0b0a0;font-size:13px}");
    out.push_str("td{padding:5px 10px;border:1px solid #d0d0c8;font-size:13px}");
    out.push_str("tr:nth-child(even) td{background:#fafaf7}");
    out.push_str(".m{color:#606060;font-size:12px;margin-top:12px}</style></head><body>");
    out.push_str("<h1>Bill of Materials</h1><div class=\"m\">");
    out.push_str(&esc(&proj.name));
    out.push_str("</div><table><tr><th>Qty</th><th>References</th><th>Value</th><th>Device</th><th>Package</th><th>Library</th></tr>");
    for r in rows {
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            r.qty,
            esc(&r.refdes.join(" ")),
            esc(&r.value),
            esc(&r.device),
            esc(&r.package),
            esc(&r.library),
        ));
    }
    out.push_str("</table><div class=\"m\">");
    out.push_str(&format!(
        "{} line items, {} components.</div></body></html>",
        rows.len(),
        total_parts(rows)
    ));
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
