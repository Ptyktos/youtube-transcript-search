//! Criterion benchmark for `parse_transcript_xml`.
//!
//! Synthesises srv3-format caption XMLs at several representative sizes and
//! confirms that parser throughput scales linearly with input size.
//!
//! Run with: `cargo bench -p youtube-transcript-mcp-core --bench parser`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use youtube_transcript_mcp_core::parse_transcript_xml;

fn synth_srv3(n_paras: usize, segs_per_para: usize) -> String {
    // Mirror the nested <p><s></s></p> structure used by Innertube srv3 caption tracks.
    let mut s = String::with_capacity(n_paras * segs_per_para * 64);
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<timedtext format=\"3\"><body>");
    for i in 0..n_paras {
        s.push_str(&format!(r#"<p t="{}" d="3000">"#, i * 3000));
        for j in 0..segs_per_para {
            s.push_str(&format!(r#"<s t="{}">word{}_{} </s>"#, j * 100, i, j));
        }
        s.push_str("</p>");
    }
    s.push_str("</body></timedtext>");
    s
}

fn bench_parser(c: &mut Criterion) {
    // (paragraphs, segments_per_paragraph, label)
    let configs: &[(usize, usize, &str)] = &[
        (100, 5, "tiny_14kb"),     // ~14 KB — short clip
        (1_000, 5, "small_150kb"), // ~150 KB — typical 10-min talk
        (10_000, 5, "med_1.5mb"),  // ~1.5 MB — long lecture
        (50_000, 5, "large_8mb"),  // ~8 MB — pathological upper bound
    ];

    let mut group = c.benchmark_group("parse_transcript_xml");
    for &(paras, segs, label) in configs {
        let xml = synth_srv3(paras, segs);
        group.throughput(Throughput::Bytes(xml.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &xml, |b, xml| {
            b.iter(|| {
                let out = parse_transcript_xml(black_box(xml)).expect("parse");
                black_box(out)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
