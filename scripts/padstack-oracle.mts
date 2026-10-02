// Development-only adapter: run frozen Web decoders, then compare format-neutral values.
export async function createPadstackOracle(load: (path: string) => Promise<any>, db: any, scale: number, geometry: any, layerCount: number) {
  const [{ AllegroPadstackResolver }, { AllegroPadDecoder }, { AllegroBondFingerDecoder }, { BackdrillShape }] = await Promise.all([
    load('src/lib/allegro/decoders/padstack.ts'), load('src/lib/allegro/decoders/pad.ts'),
    load('src/lib/allegro/decoders/bond-finger.ts'), load('src/lib/board/shapes/backdrill.ts'),
  ]);
  const diagnostics: string[] = [];
  const pads = new AllegroPadDecoder(geometry, scale, diagnostics);
  const stacks = new AllegroPadstackResolver((key: number) => db.get(key), layerCount, db.header.version);
  const fingers = new AllegroBondFingerDecoder(layerCount);
  const point = (p: any) => [p.x, p.y];
  const edge = (s: any) => ({ id:s.id, trackId:s.track_id, layer:s.layer === 0xffffffff ? -1 : s.layer, net:s.net,
    a:point(s.a), b:point(s.b), width:s.width,
    ...(s.arc ? {arc:{center:point(s.arc.center),radius:s.arc.radius,start:s.arc.start,sweep:s.arc.sweep}} : {}) });
  const webPad = (p: any) => ({layer:p.layer,type:p.type,width:p.width,height:p.height,offset:p.offset,
    corner:p.corner ?? 0,innerDiameter:p.innerDiameter ?? null,
    custom:p.custom ? {contours:p.custom,paths:p.customPaths} : null,
    backdrill:!!p.backdrill,backdrillBase:!!p.backdrillBase});
  const nativePad = (p: any) => ({layer:p.layer,type:p.kind,width:p.width,height:p.height,offset:point(p.offset),
    corner:p.corner,innerDiameter:p.inner_diameter,
    custom:p.custom ? {contours:p.custom.contours.map((r: any[])=>r.map(point)),paths:p.custom.paths.map((p: any[])=>p.map(edge))} : null,
    backdrill:p.backdrill,backdrillBase:p.backdrill_base});
  const regular = (stack: any) => {
    const result = [];
    for(let i=0;i<stack.LayerCount;i++) {
      const p = stack.Components[stack.NumFixedCompEntries + stack.NumCompsPerLayer*i + 2];
      const value = pads.shape(p, stack.StartLayer+i, [p.OffsetX*scale,p.OffsetY*scale], stack.Key);
      if(value) result.push(value);
    }
    return result;
  };
  const preview = (stack: any) => ({stackKey:stack.Key,startLayer:stack.StartLayer,layerCount:stack.LayerCount,
    padType:stack.PadType ?? null,regularPads:regular(stack).map(webPad),drill:pads.drill(stack)});
  const nativePreview = (p: any) => ({stackKey:p.stack_key,startLayer:p.start_layer,layerCount:p.layer_count,
    padType:p.pad_type,regularPads:p.regular_pads.map(nativePad),drill:p.drill});
  const nativeBackdrill = (d: any) => d ? ({spans:d.spans.map((s: any)=>({startLayer:s.start_layer,stopLayer:s.stop_layer,protectedLayer:s.protected_layer})),
    displayDiameter:d.display_diameter,startPadDiameter:d.start_pad_diameter,labelDiameter:d.label_diameter}) : null;
  const resolved = (r: any) => {
    if(!r) return null;
    const backdrill = r.backdrill ? {...r.backdrill,displayDiameter:r.backdrill.displayDiameter*scale,
      startPadDiameter:r.backdrill.startPadDiameter*scale,labelDiameter:r.backdrill.labelDiameter*scale} : null;
    return {preview:preview(r.stack),embeddedLayer:r.embeddedLayer ?? null,regionCode:r.regionCode ?? null,die:!!r.die,
      backdrillSource:r.backdrill ?? null,backdrill,
      effectivePads:backdrill ? BackdrillShape.applyPads(regular(r.stack),backdrill).map(webPad) : null};
  };
  const nativeResolved = (r: any) => r ? ({preview:nativePreview(r.preview),embeddedLayer:r.embedded_layer,regionCode:r.region_code,die:r.die,
    backdrillSource:nativeBackdrill(r.backdrill_source),backdrill:nativeBackdrill(r.backdrill),
    effectivePads:r.effective_pads ? r.effective_pads.map(nativePad) : null}) : null;
  return {
    expected(kind: number, record: any) {
      if(kind===28) return {definition:preview(record)};
      if(kind===47) {
        return {pin:resolved(stacks.resolvePin(record.Key,record.UnknownArray[1])),via:resolved(stacks.resolveVia(record.Key,record.UnknownArray[1]))};
      }
      if(kind===50) {
        const pad=db.get(record.PadPtr);
        return {pin:resolved(pad?.type===13 ? stacks.resolvePin(pad.PadStack,record.Key) : undefined)};
      }
      if(kind===51) {
        const r=stacks.resolveVia(record.Padstack,record.Key);
        return {via:resolved(r),bondFingerAngle:r ? fingers.placement({...record,type:kind},r.stack)?.angle ?? null : null};
      }
      throw new Error('UNSUPPORTED_PADSTACK_PROBE_TYPE');
    },
    actual(kind: number, value: any) {
      if(kind===28) return {definition:nativePreview(value.definition)};
      if(kind===47) return {pin:nativeResolved(value.pin),via:nativeResolved(value.via)};
      if(kind===50) return {pin:nativeResolved(value.pin)};
      if(kind===51) return {via:nativeResolved(value.via),bondFingerAngle:value.bond_finger_angle};
      throw new Error('UNSUPPORTED_PADSTACK_PROBE_TYPE');
    },
    expectedDiagnostics() {
      return diagnostics.map(message => {
        let match = /^Padstack (\d+) 的类型 (\d+) 焊盘尺寸无效$/.exec(message);
        let code: string, key: string, args: any;
        if(match) {code='BRD_PAD_DIMENSIONS';key='import.allegro.pad_dimensions';args={stack:Number(match[1]),pad_type:Number(match[2])};}
        else if((match=/^Padstack (\d+) 的圆环内外径无效$/.exec(message))) {code='BRD_PAD_DONUT';key='import.allegro.pad_donut';args={stack:Number(match[1])};}
        else if((match=/^Padstack (\d+) 的焊盘类型 (\d+) 尚无有效几何(?:（形状引用 (\d+)）)?$/.exec(message))) {code='BRD_PAD_UNSUPPORTED';key='import.allegro.pad_unsupported';args={stack:Number(match[1]),pad_type:Number(match[2]),shape:Number(match[3] ?? 0)};}
        else throw new Error(`UNMAPPED_WEB_PAD_DIAGNOSTIC: ${message}`);
        return {code,key,args,object:args.stack,offset:db.offsets.get(args.stack) ?? 0,severity:'Warning'};
      });
    },
    actualDiagnostics(row: any) {
      if(row?.kind!=='diagnostics' || row.stage!=='padstack' || row.scene_validated!==false || !Array.isArray(row.diagnostics) || row.localized_messages?.length!==row.diagnostics.length) throw new Error('PAD_DIAGNOSTICS_INVALID');
      return row.diagnostics.map((d: any)=>({code:d.code,key:d.message.key,args:d.message.args,object:d.object,offset:d.offset,severity:d.severity}));
    },
  };
}
