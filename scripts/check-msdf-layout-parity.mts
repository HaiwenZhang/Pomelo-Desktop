// Executes the latest Web MSDF preparation and layout unchanged; writes only here.
import { readFileSync,writeFileSync,mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
const [webRoot,probe,source,output]=process.argv.slice(2);
if(!output)throw Error('MSDF_PARITY_USAGE: WEB_ROOT PROBE SOURCE OUTPUT_DIR');
const load=(p:string)=>import(pathToFileURL(resolve(webRoot,p)).href);
const [{AllegroParser},{AllegroSceneBuilder},{MsdfFont},{BoardTextGlyphBuilder},{BoardLabelLayout},{BoardDisplay},{Camera}]=await Promise.all([
 load('src/lib/allegro/parser.ts'),load('src/lib/allegro/scene-builder.ts'),load('src/lib/text/msdf-font.ts'),load('src/lib/text/board-text-glyph-builder.ts'),load('src/lib/render/board-label-layout.ts'),load('src/lib/board/display.ts'),load('src/lib/interaction/camera.ts')]);
const bytes=readFileSync(source);
const scene=await new AllegroSceneBuilder(await new AllegroParser(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),'utf-8').parse()).build();
const texts:any[]=[];
for(const [i,text] of ['USB_GND','PCB 铜箔 中文 Ω','日本語\r\nカタカナ','?🚀\tPad'].entries())for(const align of ['left','center','right'])for(const mirrored of [false,true])for(const angle of [0,.47])texts.push({id:texts.length+1,layer:i%2,classId:0,subclass:0,ownerId:undefined,text,at:[123456.000123+i,-98765.003+i],angle,mirrored,align,fontIndex:0,width:.37,height:.61,spacing:.05,lineSpacing:.9,strokeWidth:.1});
const allTexts=[...texts,...scene.texts,...[...scene.nets.values()].map(text=>({text}))];
await MsdfFont.prepare(allTexts,undefined,async(block:number)=>JSON.parse(readFileSync(resolve(webRoot,`public/fonts/source-han-sans/${block.toString(16).padStart(2,'0')}.json`),'utf8')));
const display=BoardDisplay.createDisplayOptions(scene.drawingLayers),views:any[]=[],webLabels:any[]=[];
const ox=(scene.bounds.minX+scene.bounds.maxX)/2,oy=(scene.bounds.minY+scene.bounds.maxY)/2;
for(const scale of [3,30,300])for(const flipped of [false,true])for(const [width,height] of [[800,600],[1600,900]]){
 const camera=new Camera();camera.scale=scale;camera.flipped=flipped;
 views.push({camera:{center:{x:ox,y:oy},pixels_per_mm:scale,flipped},width,height,display:{active_layer:null,priorities:[],layer_order:[],hidden_layers:[...display.hidden],layer_primitives:{},show_drills:true,show_copper:true,show_drawings:true,show_texts:false,copper_opacity:display.shapes,filled:display.filled,color_mode:'layer',length_unit:'millimeters'}});
 const batches=BoardLabelLayout.layout({scene,font:MsdfFont.font,camera,width,height,options:display});
 // Desktop retains absolute board positions; Web batches use the scene's center as origin.
 webLabels.push(Object.fromEntries([...batches].map(([key,values]:any)=>[key,Array.from({length:values.length/16},(_,i)=>{const v=values.slice(i*16,i*16+16);v[0]+=ox;v[1]+=oy;return v;})])));
}
mkdirSync(output,{recursive:true});
const input=resolve(output,'request.json'),native=resolve(output,'native.json');
writeFileSync(input,JSON.stringify({source:resolve(source),texts:texts.map(t=>({id:t.id,owner_id:null,layer:t.layer,class_id:0,subclass:0,text:t.text,at:{x:t.at[0],y:t.at[1]},angle:t.angle,mirrored:t.mirrored,align:t.align,font_index:0,width:t.width,height:t.height,spacing:t.spacing,line_spacing:t.lineSpacing,stroke_width:t.strokeWidth})),views}));
const result=spawnSync(resolve(probe),[input,native],{encoding:'utf8',maxBuffer:1024*1024});if(result.status!==0)throw Error(`MSDF_NATIVE_FAILED: ${result.stderr}`);
const actual=JSON.parse(readFileSync(native,'utf8')),differences:any[]=[];let checked=0;
const compare=(key:string,a:number[][],b:number[][])=>{
 checked+=Math.max(a.length,b.length);
 if(a.length!==b.length){differences.push({key,expected:a.length,actual:b.length});return;}
 for(let i=0;i<a.length;i++)for(let j=0;j<16;j++){
  // Positions retain high+residual; all other fields cross a float32 GPU ABI.
  const tolerance=j<2?1e-7:Math.max(1e-6,Math.abs(a[i][j])*2e-7);
  if(Math.abs(a[i][j]-b[i][j])>tolerance){differences.push({key,glyph:i,field:j,expected:a[i][j],actual:b[i][j]});break;}
 }
};
for(const text of texts)compare(`text:${text.id}`,BoardTextGlyphBuilder.build(text).map((g:any)=>g.values),actual.board.filter((g:any)=>g.id===text.id).map((g:any)=>g.packet));
const category=['outline','drawing','zone','zone-outline','etch','text','pin','via','drill'];
for(let i=0;i<views.length;i++){
 const nativeBatches:Record<string,number[][]>={};
 for(const glyph of actual.labels[i]){const c=category[glyph.ids[1]];const key=c==='zone'?`zone:${glyph.ids[0]}`:c==='drill'?'drill':`${c}:${glyph.ids[2]}`;(nativeBatches[key]??=[]).push(glyph.packet);}
 for(const key of new Set([...Object.keys(webLabels[i]),...Object.keys(nativeBatches)]))compare(`view:${i}/${key}`,webLabels[i][key]??[],nativeBatches[key]??[]);
}
const sources=['src/lib/text/msdf-font.ts','src/lib/text/board-text-glyph-builder.ts','src/lib/render/font-metrics.ts','src/lib/render/board-label-layout.ts','src/lib/render/label.wgsl','public/fonts/source-han-sans/manifest.json'];
const report={source:resolve(source),sha256:createHash('sha256').update(bytes).digest('hex'),webSources:sources.map(path=>({path,sha256:createHash('sha256').update(readFileSync(resolve(webRoot,path))).digest('hex')})),syntheticTexts:texts.length,views:views.length,checkedGlyphs:checked,differences:differences.length,details:differences.slice(0,100)};
writeFileSync(resolve(output,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({...report,webSources:undefined,details:undefined}));if(differences.length)process.exitCode=1;
