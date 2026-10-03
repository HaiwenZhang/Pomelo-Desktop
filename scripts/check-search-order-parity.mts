// Execute the actual, unmodified Web search class with controlled runtime locales.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [webRoot,probe,source,outputDir,encoding='synthetic']=process.argv.slice(2);
if(!outputDir)throw Error('SEARCH_ORDER_PARITY_USAGE');
mkdirSync(outputDir,{recursive:true});
const load=(path:string)=>import(pathToFileURL(resolve(webRoot,path)).href);
const {BoardSearchIndex}=await load('src/lib/board/search.ts');
let entries:any[];
if(encoding==='synthetic') {
 // All names share searchable ASCII text, but contain accents, combining marks,
 // CJK, kana, Hangul, punctuation, digits and supplementary-plane characters.
 const variants=['','2','10','02','100','_','-',' ','/','+','.',',','①','１',
  'A','a','Ä','Å','á','é','e\u0301','ß','ss','İ','i\u0307','I','Ω','ΟΣ','ος',
  '中','国','國','电','電','线','線','阿','八','山','水','简','簡','繁','體','体',
  'あ','ア','ｱ','ぁ','ァ','は','ば','ぱ','ハ','バ','パ','か','カ','が','ガ',
  'が','か\u3099','한국','한글','가','각','나','하','가','ㄱ','힣','😀','🧪','\u{20000}',
  '\u0085','\ufeff','\u00a0','\u200b'];
 const names=variants.flatMap(text=>[`signal${text}`,`${text}signal`,`SIGnal${text}`]);
 names.push('signal','SIGNAL','Signal','SIGNAL','전원_GND','電源_GND','电源_GND','電源電圧_GND');
 let seed=9137;
 for(let i=names.length-1;i>0;i--){seed=(Math.imul(seed,1664525)+1013904223)>>>0;
  const j=seed%(i+1);[names[i],names[j]]=[names[j],names[i]];}
 entries=names.map((name,index)=>({kind:index%3?'net':'component',
  id:index%3?index+1:name,name,count:index+1}));
} else {
 const [{AllegroParser},{AllegroSceneBuilder}]=await Promise.all([
  load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts')]);
 const bytes=readFileSync(source);
 const database=await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),encoding).parse();
 entries=BoardSearchIndex.buildItems(await new AllegroSceneBuilder(database).build());
}
const locales=['en','zh-CN','zh-TW','ja','ko'];
const words=['',' ','signal',' SIGNAL ','sig','gnd','u','u1','u10','j','1','2','电','電','한국',
 'ア','あ','가','é','e\u0301','\ufeffsignal\ufeff','\u0085signal\u0085','\u00a0signal\u00a0','missing'];
for(const entry of entries.slice(0,160)) {
 words.push(entry.name,entry.name.toUpperCase(),entry.name.slice(0,2),entry.name.slice(-2));
}
const queries=[...new Set(words)].flatMap(query=>[0,1,2,7,20,entries.length+5].map(limit=>({query,limit})));
const input={locales,queries,entries};
const inputPath=resolve(outputDir,'input.json');
writeFileSync(inputPath,JSON.stringify(input));
const nativePath=resolve(outputDir,'native.json');
const run=spawnSync(resolve(probe),[resolve(source),encoding,inputPath,nativePath],{stdio:['ignore','pipe','pipe']});
if(run.status!==0)throw Error(`SEARCH_ORDER_NATIVE_PROBE ${run.status}: ${run.stderr?.toString()}`);
const native=JSON.parse(readFileSync(nativePath,'utf8'));
const normalized=(value:any):any=>Array.isArray(value)?value.map(normalized):value!==null&&typeof value==='object'
 ?Object.fromEntries(Object.keys(value).sort().map(key=>[key,normalized(value[key])])):value;
const same=(a:any,b:any)=>JSON.stringify(normalized(a))===JSON.stringify(normalized(b));
const lower=String.prototype.toLocaleLowerCase,compare=String.prototype.localeCompare;
const defaultLocale=new Intl.Collator().resolvedOptions().locale;
let total=0,legacyDifferences=0;
const expected:any[]=[];
const differences:any[]=[];
if(!same(native.entries,entries))differences.push({kind:'entries'});
try {
 for(const locale of locales) {
  // The Web class does not accept a locale. Only supply its JS runtime default;
  // leave its matching, ranking, heap, source identities and tie-breaking intact.
  String.prototype.toLocaleLowerCase=function(){return lower.call(this,locale);};
  String.prototype.localeCompare=function(other:string){return compare.call(this,other,locale);};
  const index=new BoardSearchIndex(entries);
  const positions=new Map(entries.map((entry,ordinal)=>[entry,ordinal]));
  const results=queries.map(request=>({...request,
   results:index.find(request.query,request.limit).map((entry:any)=>positions.get(entry))}));
  expected.push({locale,queries:results});
  const actual=native.locales.find((value:any)=>value.locale===locale);
  for(let q=0;q<results.length;q++) {
   total++;
   if(!same(results[q],actual?.queries[q]))differences.push({locale,
    query:results[q].query,limit:results[q].limit,expected:results[q].results,actual:actual?.queries[q]?.results});
   const text=lower.call(results[q].query.trim(),locale);
   if(text&&results[q].limit>0){
    const rank=(name:string)=>name===text?0:name.startsWith(text)?1:2;
    const old=entries.map((entry,ordinal)=>({name:lower.call(entry.name,locale),ordinal}))
     .filter(entry=>entry.name.includes(text)).sort((a,b)=>rank(a.name)-rank(b.name)
      ||Buffer.compare(Buffer.from(a.name),Buffer.from(b.name))||a.ordinal-b.ordinal)
     .slice(0,results[q].limit).map(entry=>entry.ordinal);
    if(JSON.stringify(old)!==JSON.stringify(results[q].results))legacyDifferences++;
   }
  }
 }
} finally {String.prototype.toLocaleLowerCase=lower;String.prototype.localeCompare=compare;}
// Also exercise the actual runtime defaults without any prototype adapter.
const defaultIndex=new BoardSearchIndex(entries),positions=new Map(entries.map((entry,ordinal)=>[entry,ordinal]));
const locale=locales.find(value=>defaultLocale===value||defaultLocale.startsWith(`${value}-`));
if(locale){const baseline=expected.find(value=>value.locale===locale);
 for(let q=0;q<queries.length;q++) {
  const actual=defaultIndex.find(queries[q].query,queries[q].limit).map((entry:any)=>positions.get(entry));
  if(JSON.stringify(actual)!==JSON.stringify(baseline.queries[q].results))differences.push({kind:'runtime_default',q});
 }}
writeFileSync(resolve(outputDir,'web.json'),JSON.stringify({entries,locales:expected}));
const sha=(path:string)=>createHash('sha256').update(readFileSync(path)).digest('hex');
const report={source:encoding==='synthetic'?'generated_multilingual_names':resolve(source),encoding,
 entries:entries.length,queries:queries.length,comparisons:total,mismatches:differences.length,
 legacy_byte_order_differences:legacyDifferences,differences,locales,
 runtime:{node:process.versions.node,icu:process.versions.icu,cldr:process.versions.cldr,
  unicode:process.versions.unicode,default_collator:new Intl.Collator().resolvedOptions()},
 native_query_microseconds:native.locales.map((value:any)=>({locale:value.locale,microseconds:value.query_microseconds})),
 fingerprints:{web_search:sha(resolve(webRoot,'src/lib/board/search.ts')),probe:sha(resolve(probe)),
  input:sha(inputPath),...(encoding==='synthetic'?{}:{board:sha(resolve(source))})}};
writeFileSync(resolve(outputDir,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({...report,differences:report.differences.slice(0,5)}));
if(differences.length)process.exitCode=1;
