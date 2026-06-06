const CAT = {frame:'#5b6e74',cmd:'#8fb0ff',time:'#39d6ff',hr:'#ff5a6e',rr:'#ff9f43',
  accel:'#b6ff4d',gyro:'#c08bff',ppg:'#2ee6c6',battery:'#ffd23f',event:'#5b9bff',
  meta:'#ff6fd0',text:'#7CFFB2',unknown:'#46585e'};
const CMDS = ['GET_BATTERY_LEVEL','GET_CLOCK','REPORT_VERSION_INFO','GET_HELLO_HARVARD',
  'GET_DATA_RANGE','GET_BODY_LOCATION_AND_STATUS','GET_EXTENDED_BATTERY_INFO','GET_LED_DRIVE',
  'GET_TIA_GAIN','GET_BIAS_OFFSET','GET_ALARM_TIME','GET_ALL_HAPTICS_PATTERN','LINK_VALID'];
const TYPE_ORDER = ['REALTIME_DATA','REALTIME_RAW_DATA','HISTORICAL_DATA','EVENT',
  'COMMAND_RESPONSE','METADATA','CONSOLE_LOGS'];

const $ = s => document.querySelector(s);
const latest = {};        // typeName -> packet rec
const counts = {};        // typeName -> count
let selected = null;      // pinned type
let pinned = false;
let hr = [], pktTimes = [], lastInspector = 0, selField = -1;

const ws = new WebSocket(`ws://${location.host}/ws`);
ws.onopen = () => sys('socket open');
ws.onclose = () => { sys('socket closed — reconnecting in 2s'); setTimeout(()=>location.reload(),2000); };
ws.onmessage = e => handle(JSON.parse(e.data));

function handle(m){
  if(m.kind==='hello'||m.kind==='state'){ if(m.state) renderState(m.state); if(m.categories) renderLegend(m.categories); }
  else if(m.kind==='log'){ sys(m.msg); }
  else if(m.kind==='packet'){ onPacket(m.packet); if(m.state) renderState(m.state); }
}

function onPacket(p){
  const t = p.type_name || 'UNKNOWN';
  latest[t] = p; counts[t] = (counts[t]||0)+1;
  pktTimes.push(performance.now()); if(pktTimes.length>200) pktTimes.shift();
  // HR + sparkline
  const v = p.parsed||{};
  if('heart_rate' in v && v.heart_rate){ $('#st-hr').textContent=v.heart_rate; hr.push(v.heart_rate); if(hr.length>60)hr.shift(); drawSpark(); }
  streamRow(p);
  if(!pinned){ if(!selected || (t!=='COMMAND_RESPONSE'&&t!=='CONSOLE_LOGS')) selected = preferType(); }
  renderTabs();
  const now = performance.now();
  if(p.type_name===selected && now-lastInspector>250){ lastInspector=now; renderInspector(latest[selected]); }
}

function preferType(){
  for(const t of TYPE_ORDER) if(latest[t]) return t;
  return Object.keys(latest)[0];
}

/* ---------- status ---------- */
function renderState(s){
  const c=$('#st-conn'); c.innerHTML = `<i class="dot ${s.connected?'live':''}"></i> ${s.connected?'LIVE':'down'}`;
  $('#st-dev').textContent = s.device||'—';
  $('#st-bond').textContent = s.bonded?'bonded ✓':'—';
  $('#st-fw').textContent = s.fw||'—';
  if(s.battery!=null){ $('#st-batt').textContent = s.battery+'%'; $('#batt-fill').style.width=Math.max(3,s.battery)+'%'; }
}

/* ---------- tabs ---------- */
function renderTabs(){
  const seen = Object.keys(latest).sort((a,b)=>(TYPE_ORDER.indexOf(a)+99)%100-(TYPE_ORDER.indexOf(b)+99)%100);
  $('#type-tabs').innerHTML = seen.map(t=>
    `<span class="tab ${t===selected?'active':''}" data-t="${t}">${t}<span class="n">${counts[t]}</span></span>`).join('');
  document.querySelectorAll('.tab').forEach(el=>el.onclick=()=>{
    selected=el.dataset.t; pinned=true; selField=-1; renderTabs(); renderInspector(latest[selected]); });
}

/* ---------- hex inspector ---------- */
function renderInspector(p){
  if(!p){ $('#hexgrid').innerHTML=''; return; }
  const raw = hexToBytes(p.raw);
  const fields = p.fields||[];
  const byteField = new Int16Array(raw.length).fill(-1);
  fields.forEach((f,i)=>{ for(let o=f.off;o<f.off+f.len&&o<raw.length;o++) byteField[o]=i; });
  const COLS=16; let html='';
  for(let r=0;r<raw.length;r+=COLS){
    let bytes='',ascii='';
    for(let c=0;c<COLS;c++){
      const o=r+c;
      if(o>=raw.length){ bytes+=`<span class="byte gap"> </span>`; continue; }
      const fi=byteField[o]; const cat = fi>=0?(fields[fi].cat||'unknown'):'unknown';
      const col=CAT[cat]||CAT.unknown;
      bytes+=`<span class="byte" data-field="${fi}" style="color:${col};background:${col}1a">${raw[o].toString(16).padStart(2,'0')}</span>`;
      const ch=raw[o]; ascii+= (ch>=32&&ch<127)?String.fromCharCode(ch):'·';
    }
    html+=`<div class="hexrow"><span class="hoff">0x${r.toString(16).padStart(4,'0')}</span><span class="hbytes">${bytes}</span><span class="hascii">${ascii}</span></div>`;
  }
  $('#hexgrid').innerHTML=html;
  $('#pkt-meta').textContent = `${p.type_name} · ${raw.length}B · crc ${p.crc_ok===false?'✗':(p.crc_ok?'✓':'—')} · ${p.char}`;
  renderFields(p);
  applyFieldSel();
}
function applyFieldSel(){
  document.querySelectorAll('.byte.field-sel').forEach(b=>b.classList.remove('field-sel','hl'));
  if(selField<0) return;
  document.querySelectorAll(`.byte[data-field="${selField}"]`).forEach(b=>b.classList.add('field-sel','hl'));
}

/* hover highlight via delegation */
const tip=$('#tooltip');
$('#hexgrid').addEventListener('mouseover',e=>{
  const b=e.target.closest('.byte'); if(!b||b.dataset.field===undefined) return;
  const fi=+b.dataset.field; if(fi<0){ return; }
  const p=latest[selected]; const f=p.fields[fi];
  document.querySelectorAll(`.byte[data-field="${fi}"]`).forEach(x=>x.classList.add('hl'));
  document.querySelectorAll('.frow').forEach((row,i)=>row.classList.toggle('sel',i===fi));
  tip.innerHTML=`<div class="tt-name">${f.name}</div><div class="tt-val">${fmtVal(f.value)}</div>`+
    (f.note?`<div class="tt-note">${f.note}</div>`:'')+
    `<div class="tt-off">off 0x${f.off.toString(16)} · ${f.len}B · ${f.cat}</div>`;
  tip.classList.add('show');
});
$('#hexgrid').addEventListener('mousemove',e=>{ tip.style.left=Math.min(e.clientX+14,innerWidth-300)+'px'; tip.style.top=(e.clientY+16)+'px'; });
$('#hexgrid').addEventListener('mouseout',e=>{
  const b=e.target.closest('.byte'); if(!b)return; const fi=+b.dataset.field;
  if(fi!==selField) document.querySelectorAll(`.byte[data-field="${fi}"]`).forEach(x=>x.classList.remove('hl'));
  tip.classList.remove('show'); document.querySelectorAll('.frow.sel').forEach(r=>r.classList.remove('sel'));
});

/* ---------- field readout ---------- */
function renderFields(p){
  const f=p.fields||[];
  $('#fields').innerHTML = f.map((x,i)=>{
    const col=CAT[x.cat]||CAT.unknown;
    return `<div class="frow" data-i="${i}"><span class="swatch" style="background:${col}"></span>`+
      `<span class="fname">${x.name}</span><span class="fval" title="${fmtVal(x.value)}">${fmtVal(x.value)}</span>`+
      (x.note?`<span class="fnote">${x.note}</span>`:'')+`</div>`;
  }).join('');
  document.querySelectorAll('.frow').forEach(row=>{
    row.onmouseenter=()=>{ const i=+row.dataset.i; document.querySelectorAll(`.byte[data-field="${i}"]`).forEach(b=>b.classList.add('hl')); };
    row.onmouseleave=()=>{ const i=+row.dataset.i; if(i!==selField) document.querySelectorAll(`.byte[data-field="${i}"]`).forEach(b=>b.classList.remove('hl')); };
    row.onclick=()=>{ selField=+row.dataset.i; applyFieldSel(); };
  });
  const pv=p.parsed||{};
  $('#parsed').innerHTML = Object.keys(pv).length? Object.entries(pv).map(([k,v])=>
    `<div class="kv"><b>${k}</b><span>${fmtVal(v)}</span></div>`).join('') :
    `<div class="kv"><b>no parsed fields</b><span>raw only</span></div>`;
}

/* ---------- legend ---------- */
function renderLegend(cats){
  $('#legend').innerHTML = cats.map(c=>`<span class="lg"><i style="background:${CAT[c]||CAT.unknown}"></i>${c}</span>`).join('');
}

/* ---------- stream log ---------- */
function streamRow(p){
  const v=p.parsed||{}; let info='';
  if('heart_rate' in v) info=`HR ${v.heart_rate}`+ (v.rr_intervals&&v.rr_intervals.length?` rr ${v.rr_intervals.join(',')}`:'');
  else if(v.log) info=v.log.slice(0,70);
  else if(v.battery_pct!=null) info=`batt ${v.battery_pct}%`;
  else if(p.type_name==='EVENT'){ const ev=(p.fields||[]).find(f=>f.name==='event'); info=ev?ev.value:''; }
  else if(p.cmd_name) info=p.cmd_name;
  const t=new Date(p.ts*1000).toLocaleTimeString('en',{hour12:false});
  const col=CAT[(p.fields&&p.fields[3]&&p.fields[3].cat)]||'#9fb';
  addLog(`<span class="lt">${t}</span><span class="ltype" style="color:${typeColor(p.type_name)}">${p.type_name}</span>`+
    `<span class="lchar">${p.char}</span><span class="lsz">${p.len_bytes}B</span><span class="linfo">${info}</span>`,'');
}
function typeColor(t){return ({REALTIME_DATA:CAT.hr,REALTIME_RAW_DATA:CAT.accel,HISTORICAL_DATA:CAT.meta,
  EVENT:CAT.event,COMMAND_RESPONSE:CAT.cmd,METADATA:CAT.meta,CONSOLE_LOGS:CAT.text})[t]||CAT.unknown;}
function sys(msg){ addLog(`<span class="lt">${new Date().toLocaleTimeString('en',{hour12:false})}</span><span class="linfo">${msg}</span>`,'sys'); }
function addLog(html,cls){ const l=$('#log'); const d=document.createElement('div'); d.className='lrow '+cls; d.innerHTML=html;
  l.prepend(d); while(l.children.length>120) l.lastChild.remove(); }

/* ---------- sparkline ---------- */
function drawSpark(){
  const c=$('#spark'),x=c.getContext('2d'),W=c.width,H=c.height; x.clearRect(0,0,W,H);
  if(hr.length<2)return; const mn=Math.min(...hr)-2,mx=Math.max(...hr)+2;
  x.beginPath(); hr.forEach((v,i)=>{const px=i/(hr.length-1)*W, py=H-(v-mn)/(mx-mn)*H; i?x.lineTo(px,py):x.moveTo(px,py);});
  x.strokeStyle=CAT.hr; x.lineWidth=1.5; x.shadowColor=CAT.hr; x.shadowBlur=6; x.stroke();
}

/* ---------- controls ---------- */
document.querySelectorAll('.deck .btn[data-act]').forEach(b=>b.onclick=()=>send({action:b.dataset.act}));
$('#cmd-sel').innerHTML = CMDS.map(c=>`<option>${c}</option>`).join('');
$('#cmd-send').onclick=()=>send({action:'cmd',name:$('#cmd-sel').value,payload:'00'});
function send(o){ if(ws.readyState===1) ws.send(JSON.stringify(o)); }

/* rate meter */
setInterval(()=>{ const now=performance.now(); const n=pktTimes.filter(t=>now-t<1000).length; $('#rate').textContent=n+' pkt/s'; },1000);

/* ---------- utils ---------- */
function hexToBytes(h){ const a=new Uint8Array(h.length/2); for(let i=0;i<a.length;i++)a[i]=parseInt(h.substr(i*2,2),16); return a; }
function fmtVal(v){ if(v==null)return '—'; if(Array.isArray(v))return '['+v.join(', ')+']'; return String(v); }
