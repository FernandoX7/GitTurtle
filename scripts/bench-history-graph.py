#!/usr/bin/env python3
"""Measure paged graph preparation on an existing disposable history fixture.

Extracts graph.rs, compiles it with rustc -O, and supplies the fixture's first
100,000 topological commit/parent identities. Parsing precedes timing. Includes
budget preflight; excludes parsing, output destruction, Git, GPUI and native frames.
"""
import argparse, hashlib, json, statistics, subprocess, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    source=(ROOT/'crates/app/src/graph.rs').read_text()
    engine=source[source.index('#[derive(Clone, Debug, PartialEq, Eq)]'):source.index('/// A shared horizontal lane viewport')]
    harness='''#![allow(dead_code)]
use std::{collections::{HashMap,HashSet},io::{self,Read},sync::Arc,time::Instant,hint::black_box};
pub struct Commit { oid:String,parents:Vec<String> }
'''+engine+'''
fn main() {
    let mut input=String::new(); io::stdin().read_to_string(&mut input).unwrap();
    let commits:Vec<_>=input.lines().map(|line|{let mut fields=line.split_whitespace();Commit{oid:fields.next().unwrap().into(),parents:fields.map(str::to_owned).collect()}}).collect();
    for sample in 0..5 {
        for mode in ["prefix","incremental"] {
            let mut cursor=GraphCursor::default();
            for offset in (0..commits.len()).step_by(500) {
                let start=Instant::now();
                let (rows,hidden)=if mode=="prefix" { GraphCursor::default().append(black_box(&commits[..offset+500]),128,200000,||Ok::<_,()>(())).unwrap() } else { cursor.append(black_box(&commits[offset..offset+500]),128,200000,||Ok::<_,()>(())).unwrap() };
                black_box(&rows);
                let elapsed=start.elapsed().as_secs_f64()*1000.;
                println!("{mode},{sample},{offset},{elapsed:.6},{hidden}");
                if sample==0 && offset==0 {
                    let start=Instant::now();
                    for _ in 0..200 {for row in rows.iter().take(60){black_box(row.clone());}}
                    println!("scroll_clone,{sample},{offset},{:.6},{hidden}",start.elapsed().as_secs_f64()*1000./200.);
                }
            }
        }
    }
}
'''
    graph=subprocess.check_output(['git','-C',str(args.fixture),'log','--topo-order','--all','--max-count=100000','--format=%H %P'],text=True)
    with tempfile.TemporaryDirectory(prefix='gitturtle-history-graph-') as temp:
        path=Path(temp); rust=path/'graph.rs'; executable=path/'graph';rust.write_text(harness)
        subprocess.run(['rustc','-O','--edition','2024',str(rust),'-o',str(executable)],check=True)
        raw=subprocess.check_output([str(executable)],input=graph,text=True)
    args.output.write_text(raw)
    (args.output.with_suffix('.provenance.json')).write_text(json.dumps({'graph_source_sha256':hashlib.sha256(source.encode()).hexdigest(),'harness_sha256':hashlib.sha256(harness.encode()).hexdigest(),'fixture_identities_sha256':hashlib.sha256(graph.encode()).hexdigest(),'commits':len(graph.splitlines()),'samples':5,'scope':'Graph preparation/preflight only; no Git or UI frame timings. Scroll_clone measures 60 Arc row clones averaged over 200 simulated frames; excludes rendering.'},indent=2)+'\n')

if __name__=='__main__':main()
