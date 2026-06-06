/* WHOOP·DECODE — datastore dashboard. Reads the whoop-ingest REST API and renders
   stored history. PRIMARY content = decoded biometrics (HR/battery + type-47 SpO2/skin/
   resp/gravity + events). The batch browser + hex drill-down (re-parses archived frames)
   are now a SECONDARY, collapsible "Raw archive · research" section that auto-hides when
   the device has 0 raw batches — raw is only captured when research-raw mode is enabled in
   the app. Aesthetic + hex/field rendering carried over from the original live BLE dashboard. */
const CAT = {frame:'#5b6e74',cmd:'#8fb0ff',time:'#39d6ff',hr:'#ff5a6e',rr:'#ff9f43',
  accel:'#b6ff4d',gyro:'#c08bff',ppg:'#2ee6c6',battery:'#ffd23f',event:'#5b9bff',
  meta:'#ff6fd0',text:'#7CFFB2',unknown:'#46585e'};
// Type-47 biometric series colors (reuse the live-dashboard palette).
const BIO = {red:'#ff5a6e', ir:'#ff9f43', skin:'#ffd23f', resp:'#2ee6c6',
  gx:'#b6ff4d', gy:'#39d6ff', gz:'#c08bff'};
const CATEGORIES = ['frame','cmd','time','hr','rr','accel','gyro','ppg','battery','event','meta','text','unknown'];
// Sleep-stage palette for the hypnogram + breakdown (deep darkest → wake brightest).
const STAGE = {deep:'#3b6bff', light:'#2ee6c6', rem:'#c08bff', wake:'#ff9f43'};
const STAGE_ORDER = ['deep','light','rem','wake'];

const $ = s => document.querySelector(s);
const state = { device: null, range: 'all', selField: -1, frame: null, researchOpen: false };

/* ---------- bootstrap ---------- */
init();
async function init(){
  renderLegend(CATEGORIES);
  bindControls();
  try{
    const devs = await api('/v1/devices');
    const sel = $('#device-sel');
    if(!devs.length){ status('no devices in datastore yet — run the uploader'); return; }
    sel.innerHTML = devs.map(d=>`<option value="${d.device_id}">${d.device_id}</option>`).join('');
    state.device = devs[0].device_id;
    await loadAll();
  }catch(e){ status('error: '+e.message); }
}

function bindControls(){
  $('#device-sel').onchange = e => { state.device = e.target.value; loadAll(); };
  $('#reload').onclick = () => loadAll();
  document.querySelectorAll('#ranges .btn').forEach(b => b.onclick = () => {
    document.querySelectorAll('#ranges .btn').forEach(x=>x.classList.remove('on'));
    b.classList.add('on'); state.range = b.dataset.range; loadAll();
  });
  // Raw archive (research) is collapsible — and only shown at all when the device has raw batches.
  $('#research-toggle').onclick = () => setResearchOpen(!state.researchOpen);
}

/* ---------- raw archive · research section (secondary, collapsible, auto-hidden) ---------- */
function setResearchOpen(open){
  state.researchOpen = open;
  $('#research-body').hidden = !open;
  $('#research-toggle').setAttribute('aria-expanded', open ? 'true' : 'false');
  const caret = document.querySelector('.research-caret');
  if(caret) caret.textContent = open ? '▾' : '▸';
}
// Show the whole section only when raw batches exist; default it collapsed.
function updateResearchVisibility(batchCount){
  const has = batchCount > 0;
  $('#research').hidden = !has;
  $('#research-count').textContent = has ? `${batchCount} batch${batchCount===1?'':'es'}` : '';
  if(!has) setResearchOpen(false);
}

function rangeWindow(){
  const now = Math.floor(Date.now()/1000);
  if(state.range === 'all') return { from: 0, to: 2_000_000_000 };
  return { from: now - (+state.range), to: now };
}

/* ---------- load everything for device+range ---------- */
async function loadAll(){
  if(!state.device) return;
  status('loading…');
  const { from, to } = rangeWindow();
  const q = `device=${encodeURIComponent(state.device)}&from=${from}&to=${to}`;
  try{
    const [hr, batt, events, spo2, skin, resp, grav, batches, summary] = await Promise.all([
      // High-rate 1 Hz streams: server-side time-bucket downsampling keeps the FULL
      // range in view (latest sample at the right edge) instead of the oldest 20k.
      api(`/v1/streams/hr?${q}&max_points=2500`),
      api(`/v1/streams/battery?${q}&limit=5000`),
      api(`/v1/streams/events?${q}&limit=2000`),
      api(`/v1/streams/spo2?${q}&max_points=2500`),
      api(`/v1/streams/skin_temp?${q}&max_points=2500`),
      api(`/v1/streams/resp?${q}&max_points=2500`),
      api(`/v1/streams/gravity?${q}&max_points=2500`),
      api(`/v1/batches?device=${encodeURIComponent(state.device)}&limit=200`),
      api(`/v1/summary?${q}`),
    ]);
    $('#st-dev').textContent = state.device;
    $('#st-range').textContent = state.range==='all' ? 'all' : labelRange(state.range);
    renderSummary(hr);
    // Pre-map every stream's points so we can derive a SINGLE shared x-axis time domain across
    // all of them. Without this, each panel auto-scaled to its own min/max ts: HR (which has
    // extra live-HR points beyond the type-47 backfill frontier) spanned a wider range and
    // rendered sparse, while the narrower biometric streams looked dense — making HR look like
    // it was missing data when it isn't. One shared domain lets the panels stack and compare
    // honestly (streams that don't cover the whole span just show empty regions at the ends).
    const hrPts   = hr.map(r=>({t:tms(r.ts), v:r.bpm}));
    const battPts = batt.map(r=>({t:tms(r.ts), v:r.soc}));
    const spo2Pts = spo2.map(r=>({t:tms(r.ts), v:r.value}));
    const skinPts = skin.map(r=>({t:tms(r.ts), v:r.value}));
    const respPts = resp.map(r=>({t:tms(r.ts), v:r.value}));
    const gxPts = grav.map(r=>({t:tms(r.ts), v:r.x}));
    const gyPts = grav.map(r=>({t:tms(r.ts), v:r.y}));
    const gzPts = grav.map(r=>({t:tms(r.ts), v:r.z}));
    // Union extent across all loaded stream series (only series with data contribute).
    const dom = streamDomain([hrPts, battPts, spo2Pts, skinPts, respPts, gxPts]);
    dom.tMax = Math.max(Date.now(), dom.tMax ?? 0);   // anchor the right edge to live "now"
    dom.nowEdge = true;
    // Size the signal canvases so 24h fills the visible width (wider loaded range →
    // horizontal scroll). MUST run before the draws so sizeCanvas measures the new width.
    applySignalsWindow(dom);
    drawChart($('#hr-chart'), hrPts, CAT.hr, {fixedMin:40, ...dom});
    // Battery: dense soc% (from BATTERY_LEVEL events), charging stretches shaded, mV in tooltip.
    drawChart($('#batt-chart'), battPts, CAT.battery, {fixedMin:0, fixedMax:100, ...dom, bands: chargingBands(batt)});
    const bc = $('#batt-chart');
    if(bc && bc._hover){ bc._hover.series.push({pts: batt.map(r=>({t:tms(r.ts), v:r.mv})), color:'#7aa2ff', name:'mV', unit:'mV'}); }
    // Type-47 biometric streams. gravity (x/y/z) is multi-series (raw g);
    // spo2/skin_temp/resp now chart the APPROXIMATE human-unit `value` (°C / % / bpm)
    // the read API computes via analysis/units.py; raw ADC is still in each row.
    drawChart($('#spo2-chart'), spo2Pts, BIO.red, {...dom});
    drawChart($('#skin-chart'), skinPts, BIO.skin, {...dom});
    drawChart($('#resp-chart'), respPts, BIO.resp, {...dom});
    drawMultiChart($('#grav-chart'), [
      {pts: gxPts, color: BIO.gx, name:'x'},
      {pts: gyPts, color: BIO.gy, name:'y'},
      {pts: gzPts, color: BIO.gz, name:'z'},
    ], {...dom});
    scrollSignalsToNow();   // land at the right edge (latest 24h) after (re)drawing
    $('#sig-meta').textContent = `24h window · scroll ← · ${hr.length} hr · ${batt.length} batt`;
    $('#bio-meta').textContent = `${spo2.length} spo2 · ${skin.length} skin · ${resp.length} resp · ${grav.length} grav`;
    renderEvents(events);
    // Primary DAILY panel — derived sleep/recovery/strain (Task 2.6). Independent of the
    // streams above: it reads /v1/daily over the same window (as dates) + /v1/sleep for the
    // latest day with data. Non-fatal if it fails (keeps the streams dashboard intact).
    loadDaily().catch(e => { $('#daily-meta').textContent = 'unavailable'; });
    // SLEEP HISTORY panel — independent per-night list (last ~30d) + drill-down. Non-fatal.
    loadSleepHistory().catch(e => {
      $('#sleep-meta').textContent = 'unavailable';
      $('#sleep-body').innerHTML = `<div class="empty">sleep history unavailable</div>`;
    });
    // Raw archive is research-only: show + populate it only when the device actually has
    // raw batches (summary.batches is the authoritative, unlimited count). When 0, the whole
    // section is hidden and the decoded biometrics dashboard stands alone.
    updateResearchVisibility(summary.batches);
    if(summary.batches > 0) renderBatches(batches);
    // Accurate, unlimited totals (the row/list fetches above are capped for rendering only).
    const totalSamples = summary.hr + summary.rr + summary.events + summary.battery
      + summary.spo2 + summary.skin_temp + summary.resp + summary.gravity;
    $('#st-nhr').textContent = totalSamples;   // "Samples" = all decoded streams, matches the phone's total
    $('#st-nbatch').textContent = summary.batches;
    $('#batch-meta').textContent = batches.length < summary.batches
      ? `${summary.batches} (latest ${batches.length} shown)` : `${summary.batches}`;
    status(`ok · ${totalSamples} samples · ${summary.batches} raw batches`);
  }catch(e){ status('error: '+e.message); }
}

/* ---------- DAILY panel (sleep / recovery / strain) ---------- */
// Choose a from/to date window for /v1/daily. We always look back at least ~60d so the
// "latest day with data" is found even when the stream range is 24h; for the 'all' range
// we go back ~2 years.
function dailyDateWindow(){
  const to = new Date();
  const from = new Date(to);
  const days = state.range === 'all' ? 730 : Math.max(60, Math.ceil((+state.range)/86400));
  from.setUTCDate(from.getUTCDate() - days);
  return { from: ymd(from), to: ymd(to) };
}
function ymd(d){ return d.toISOString().slice(0,10); }

async function loadDaily(){
  const { from, to } = dailyDateWindow();
  const daily = await api(`/v1/daily?device=${encodeURIComponent(state.device)}&from=${from}&to=${to}`);
  if(!daily.length){
    $('#daily-meta').textContent = 'no derived metrics yet';
    $('#daily-body').innerHTML = `<div class="empty">no daily metrics — run /v1/compute-daily for this device</div>`;
    return;
  }
  // rows are ordered ascending by day; "latest" = last row.
  const latest = daily[daily.length-1];
  $('#daily-meta').textContent = `${daily.length} day${daily.length===1?'':'s'} · latest ${latest.day}`;
  // Pull the latest day's sleep sessions (for the hypnogram). Non-fatal if it 404s/empties.
  let sleep = [];
  try{ sleep = await api(`/v1/sleep?device=${encodeURIComponent(state.device)}&date=${encodeURIComponent(latest.day)}`); }
  catch(e){ sleep = []; }
  renderDaily(latest, daily, sleep);
}

function renderDaily(d, series, sleep){
  const recovery = numOrNull(d.recovery);
  const strain   = numOrNull(d.strain);
  const recCls   = recovery==null ? 'na' : recovery>=67 ? 'good' : recovery>=34 ? 'mid' : 'low';
  const sleepMin = numOrNull(d.total_sleep_min);
  const effPct   = d.efficiency==null ? null : Math.round(d.efficiency*100);
  const exCount  = d.exercise_count==null ? 0 : d.exercise_count;

  // New calibrated nightly values (may be null if not yet computed or no data).
  const spo2    = numOrNull(d.spo2_pct);
  const skinDev = numOrNull(d.skin_temp_dev_c);
  const respBpm = numOrNull(d.resp_rate_bpm);
  // Reset exercise detail state — the DOM is fully rebuilt below.
  _exState.openDay = null;

  // Sign-prefix helper for the skin-temp deviation (e.g. "+0.3" or "-0.2").
  const fmtDev = v => v==null ? '—' : (v>=0 ? '+' : '') + v.toFixed(1);

  $('#daily-body').innerHTML = `
    <div class="daily-grid">
      <div class="dcard sleep">
        <label>Sleep <span class="approx">≈ approx staging</span></label>
        <div class="big-metric">${sleepMin==null?'—':fmtDur(sleepMin)}<span class="unit">${effPct==null?'':effPct+'% eff'}</span></div>
        ${bedwakeLine(d) ? `<div class="dsub bedwake">${bedwakeLine(d)}</div>` : ''}
        <canvas id="hypno" class="hypno" height="34"></canvas>
        <div id="stage-legend" class="stage-legend"></div>
        <div class="dsub">${disturbLine(d)}</div>
      </div>
      <div class="dcard recovery ${recCls}">
        <label>Recovery <span class="approx">≈ approx</span></label>
        <div class="big-metric">${recovery==null?'—':recovery}<span class="unit">/100</span></div>
        <div class="dsub">resting HR ${fmtMetric(d.resting_hr,'bpm')} · HRV ${fmtMetric(d.avg_hrv,'ms')}</div>
      </div>
      <div class="dcard strain">
        <label>Strain <span class="approx">≈ approx</span></label>
        <div class="big-metric">${strain==null?'—':strain.toFixed(1)}<span class="unit">/21</span></div>
        <div class="dsub">${exCount ? `<button class="btn ex-toggle" id="ex-toggle" data-day="${d.day}">exercises ${exCount} ▸</button>` : 'exercises 0'}</div>
      </div>
      <div class="dcard biometrics">
        <label>Nightly biometrics <span class="approx">≈ uncalibrated</span></label>
        <div class="bio-row">
          <span class="bio-item">
            <span class="bio-label">SpO₂</span>
            <span class="bio-val">${spo2==null?'—':spo2.toFixed(1)}</span>
            <span class="bio-unit">%</span>
          </span>
          <span class="bio-item">
            <span class="bio-label">Skin Δ</span>
            <span class="bio-val skin-dev">${fmtDev(skinDev)}</span>
            <span class="bio-unit">°C vs baseline</span>
          </span>
          <span class="bio-item">
            <span class="bio-label">Resp</span>
            <span class="bio-val">${respBpm==null?'—':respBpm.toFixed(1)}</span>
            <span class="bio-unit">br/min</span>
          </span>
        </div>
      </div>
    </div>
    <div id="ex-detail" class="ex-detail" hidden></div>
    <div class="daily-trend">
      <div class="chart-label"><span>RECOVERY / STRAIN TREND</span><span class="chart-unit"><span class="approx">≈ approx</span></span></div>
      <canvas id="daily-trend-chart" class="chart" height="110"></canvas>
    </div>`;

  // Hypnogram (or stacked fallback) for the latest day's sleep.
  const segs = collectStages(sleep, d);
  drawHypnogram($('#hypno'), segs, d);
  renderStageLegend($('#stage-legend'), segs, d);

  // Exercise toggle: load compute-daily on click to surface per-bout intensity.
  const exBtn = $('#ex-toggle');
  if(exBtn) exBtn.onclick = () => toggleExerciseDetail(exBtn.dataset.day, exBtn);

  // Trend: recovery (phos) + strain*scaled? Keep separate series on a shared 0-100 axis;
  // strain (0-21) is plotted scaled to 0-100 for shape comparison, labeled in the legend.
  const recPts = series.filter(r=>r.recovery!=null).map(r=>({t:dayMs(r.day), v:+r.recovery}));
  const strPts = series.filter(r=>r.strain!=null).map(r=>({t:dayMs(r.day), v:+r.strain/21*100}));
  drawMultiChart($('#daily-trend-chart'), [
    {pts: recPts, color: '#39ff9e'},
    {pts: strPts, color: '#ff9f43'},
  ], {fixedMin:0, fixedMax:100});
}

/* Exercise per-bout detail: fetch /v1/compute-daily for the day and render a compact
   bout list (duration + strain + zone bar + avg %HRR). Toggling the same day collapses. */
const _exState = { openDay: null };
async function toggleExerciseDetail(day, btn){
  const panel = $('#ex-detail');
  if(!panel) return;
  if(_exState.openDay === day){
    // Collapse.
    _exState.openDay = null;
    panel.hidden = true;
    if(btn) btn.textContent = btn.textContent.replace('▾','▸');
    return;
  }
  _exState.openDay = day;
  if(btn){ btn.textContent = btn.textContent.replace('▸','▾'); }
  panel.hidden = false;
  panel.innerHTML = `<div class="empty">loading exercise sessions…</div>`;
  try{
    const res = await apiPost('/v1/compute-daily', { device: state.device, date: day });
    const exs = res.exercises || [];
    if(!exs.length){
      panel.innerHTML = `<div class="empty">no exercise sessions for ${day}</div>`;
      return;
    }
    panel.innerHTML = `<div class="ex-list">${exs.map(e => renderExerciseBout(e)).join('')}</div>`;
  }catch(err){
    panel.innerHTML = `<div class="empty">exercise detail unavailable (${err.message})</div>`;
  }
}

function renderExerciseBout(e){
  const durMin = e.duration_s != null ? (e.duration_s/60).toFixed(0) : null;
  const strain = e.strain != null ? (+e.strain).toFixed(1) : null;
  const avgHrr = e.avg_hrr_pct != null ? Math.round(e.avg_hrr_pct) : null;
  const peakHr = e.peak_hr != null ? Math.round(e.peak_hr) : null;
  // Zone bar: zones 0–5 (0=rest/light → 5=max), width ∝ time fraction.
  const zPct = e.zone_time_pct || {};
  const ZONE_COLORS = ['#3f5450','#2ee6c6','#39d6ff','#ffd23f','#ff9f43','#ff5a6e'];
  const zoneBar = [0,1,2,3,4,5].map(z=>{
    const pct = +(zPct[z] || zPct[String(z)] || 0);
    if(pct < 0.5) return '';
    return `<i style="width:${pct.toFixed(1)}%;background:${ZONE_COLORS[z]}" title="Z${z} ${pct.toFixed(0)}%"></i>`;
  }).join('');
  const kindLabel = e.kind ? `<span class="ex-kind">${e.kind}</span>` : '';
  return `<div class="ex-bout">
    ${kindLabel}
    <span class="ex-dur">${durMin==null?'—':durMin+'m'}</span>
    <span class="ex-strain">${strain==null?'—':strain}<span class="bio-unit"> strain</span></span>
    ${avgHrr!=null ? `<span class="ex-hrr">${avgHrr}%<span class="bio-unit"> avg%HRR</span></span>` : ''}
    ${peakHr!=null ? `<span class="ex-hr">${peakHr}<span class="bio-unit"> peak bpm</span></span>` : ''}
    <div class="ex-zones" title="HR zones 0–5 (rest→max)">${zoneBar}</div>
  </div>`;
}

/* ---------- SLEEP HISTORY panel (per-night list + drill-down) ---------- */
// Reuses /v1/daily (range) for the list; /v1/sleep + /v1/streams/* on drill-down. Tracks the
// expanded night so re-renders (e.g. resize) keep it open.
const sleepState = { nights: [], openDay: null };

async function loadSleepHistory(){
  // Look back ~30d (or the selected window, but at least 30d so the list isn't empty for short
  // stream ranges). Reuse dailyDateWindow but clamp to ~30d for the history list.
  const to = new Date();
  const from = new Date(to);
  const days = state.range === 'all' ? 30 : Math.max(30, Math.ceil((+state.range)/86400));
  from.setUTCDate(from.getUTCDate() - days);
  const daily = await api(`/v1/daily?device=${encodeURIComponent(state.device)}`
    + `&from=${ymd(from)}&to=${ymd(to)}`);
  // Prioritize nights WITH sleep; most-recent first.
  const nights = (daily||[]).filter(d => numOrNull(d.total_sleep_min) > 0)
    .sort((a,b)=> b.day.localeCompare(a.day));
  sleepState.nights = nights;
  if(sleepState.openDay && !nights.some(n=>n.day===sleepState.openDay)) sleepState.openDay = null;
  renderSleepHistory(nights);
}

function renderSleepHistory(nights){
  $('#sleep-meta').textContent = nights.length
    ? `${nights.length} night${nights.length===1?'':'s'} · last 30d` : '';
  if(!nights.length){
    $('#sleep-body').innerHTML =
      `<div class="empty sleep-empty">No sleep sessions yet — wear the strap overnight and sync.</div>`;
    return;
  }
  const maxMin = Math.max(...nights.map(n=> numOrNull(n.total_sleep_min)||0), 1);
  $('#sleep-body').innerHTML = nights.map(n=>{
    const mins = numOrNull(n.total_sleep_min)||0;
    const effPct = n.efficiency==null ? null : Math.round(n.efficiency*100);
    const barPct = Math.max(2, mins/maxMin*100);
    const rec = numOrNull(n.recovery), str = numOrNull(n.strain);
    const bedwake = (n.sleep_start!=null && n.sleep_end!=null)
      ? `${fmtClock(n.sleep_start)} → ${fmtClock(n.sleep_end)}` : null;
    const sub = [
      bedwake,
      n.deep_min!=null  ? `deep ${Math.round(n.deep_min)}m`   : null,
      n.rem_min!=null   ? `rem ${Math.round(n.rem_min)}m`     : null,
      n.light_min!=null ? `light ${Math.round(n.light_min)}m` : null,
      rec!=null ? `rec ${rec}` : null,
      str!=null ? `strain ${str.toFixed(1)}` : null,
    ].filter(Boolean).join(' · ');
    const open = sleepState.openDay===n.day;
    return `<div class="snight ${open?'open':''}" data-day="${n.day}">
      <div class="snrow" data-day="${n.day}">
        <span class="sncaret">${open?'▾':'▸'}</span>
        <span class="snday">${fmtNightDay(n.day)}</span>
        <span class="snbar"><i style="width:${barPct.toFixed(1)}%"></i></span>
        <span class="snhrs">${fmtDur(mins)}</span>
        <span class="sneff">${effPct==null?'—':effPct+'%'}</span>
      </div>
      <div class="snsub">${sub||'no stage breakdown'}</div>
      <div class="sndetail" id="sndetail-${n.day}" ${open?'':'hidden'}></div>
    </div>`;
  }).join('');
  document.querySelectorAll('#sleep-body .snrow').forEach(r=>{
    r.onclick = () => toggleNight(r.dataset.day);
  });
  // Re-open the previously expanded night (e.g. after a resize-triggered reload).
  if(sleepState.openDay) openNight(sleepState.openDay);
}

function toggleNight(day){
  if(sleepState.openDay===day){ sleepState.openDay=null; renderSleepHistory(sleepState.nights); return; }
  sleepState.openDay = day;
  renderSleepHistory(sleepState.nights);
}

async function openNight(day){
  const night = sleepState.nights.find(n=>n.day===day);
  const host = $(`#sndetail-${cssId(day)}`);
  if(!night || !host) return;
  host.innerHTML = `<div class="empty">loading night…</div>`;
  let sleep = [];
  try{ sleep = await api(`/v1/sleep?device=${encodeURIComponent(state.device)}&date=${encodeURIComponent(day)}`); }
  catch(e){ sleep = []; }
  // Window = union of the night's sessions (fall back to the calendar day if no sessions).
  let from, to;
  const bounds = (sleep||[]).map(s=>({s: tsec(s.start_ts), e: tsec(s.end_ts)}))
    .filter(b=>Number.isFinite(b.s)&&Number.isFinite(b.e));
  if(bounds.length){
    from = Math.min(...bounds.map(b=>b.s));
    to   = Math.max(...bounds.map(b=>b.e));
  } else {
    from = Math.floor(new Date(day+'T00:00:00Z').getTime()/1000);
    to   = from + 86400;
  }
  // Per-session efficiency / resting HR / avg HRV (first session w/ data) + stage segments.
  const sess = (sleep||[])[0] || {};
  const segs = collectStages(sleep, night);
  const stageMins = {};
  segs.forEach(s=>{ stageMins[s.stage] = (stageMins[s.stage]||0) + (s.end-s.start)/60; });
  const effPct = sess.efficiency!=null ? Math.round(sess.efficiency*100)
                 : (night.efficiency!=null ? Math.round(night.efficiency*100) : null);
  const rhr = sess.resting_hr!=null ? sess.resting_hr : night.resting_hr;
  const hrv = sess.avg_hrv!=null ? sess.avg_hrv : night.avg_hrv;
  const stageLine = STAGE_ORDER.filter(k=>stageMins[k]!=null)
    .map(k=>`<span class="slg"><i style="background:${STAGE[k]}"></i>${k} ${Math.round(stageMins[k])}m</span>`).join('')
    || disturbLine(night);

  host.innerHTML = `
    <div class="snd-head">
      <span class="snd-metric">eff <b>${effPct==null?'—':effPct+'%'}</b></span>
      <span class="snd-metric">resting HR <b>${fmtMetric(rhr,'bpm')}</b></span>
      <span class="snd-metric">avg HRV <b>${fmtMetric(hrv,'ms')}</b></span>
    </div>
    <div class="chart-label"><span>HYPNOGRAM</span><span class="chart-unit"><span class="approx">≈ approx staging</span></span></div>
    <canvas id="hypno-${day}" class="hypno" height="34"></canvas>
    <div class="stage-legend">${stageLine}</div>
    <div class="snd-charts">
      <div class="chart-wrap"><div class="chart-label"><span>HEART RATE</span><span class="chart-unit">bpm</span></div>
        <canvas id="snd-hr-${day}" class="chart" height="80"></canvas></div>
      <div class="chart-wrap"><div class="chart-label"><span>HRV (RR successive Δ)</span><span class="chart-unit">ms</span></div>
        <canvas id="snd-hrv-${day}" class="chart" height="80"></canvas></div>
      <div class="chart-wrap"><div class="chart-label"><span>RESPIRATION</span><span class="chart-unit">bpm <span class="approx">≈ approx</span></span></div>
        <canvas id="snd-resp-${day}" class="chart" height="80"></canvas></div>
      <div class="chart-wrap"><div class="chart-label"><span>SpO₂</span><span class="chart-unit">% <span class="approx">≈ approx</span></span></div>
        <canvas id="snd-spo2-${day}" class="chart" height="80"></canvas></div>
      <div class="chart-wrap"><div class="chart-label"><span>MOVEMENT</span><span class="chart-unit">Δg <span class="approx">L2 of gravity Δ</span></span></div>
        <canvas id="snd-move-${day}" class="chart" height="80"></canvas></div>
    </div>
    <button class="btn snd-close" data-day="${day}">▴ collapse</button>`;

  // Hypnogram for this night.
  drawHypnogram($(`#hypno-${cssId(day)}`), segs, night);
  $(`#sndetail-${cssId(day)} .snd-close`).onclick = () => toggleNight(day);

  // That night's 1 Hz traces, windowed to from→to (unix seconds).
  const q = `device=${encodeURIComponent(state.device)}&from=${from}&to=${to}&limit=20000&max_points=2500`;
  try{
    const [hr, rr, resp, spo2, grav] = await Promise.all([
      api(`/v1/streams/hr?${q}`),
      api(`/v1/streams/rr?${q}`),
      api(`/v1/streams/resp?${q}`),
      api(`/v1/streams/spo2?${q}`),
      api(`/v1/streams/gravity?${q}`),
    ]);
    drawChart($(`#snd-hr-${cssId(day)}`), hr.map(r=>({t:tms(r.ts), v:r.bpm})), CAT.hr, {fixedMin:40});
    // HRV via successive RR differences (|RR[i]-RR[i-1]| in ms) — quick beat-to-beat variability.
    const hrvPts = [];
    for(let i=1;i<rr.length;i++){ const d=Math.abs(rr[i].rr_ms - rr[i-1].rr_ms);
      if(Number.isFinite(d)) hrvPts.push({t:tms(rr[i].ts), v:d}); }
    drawChart($(`#snd-hrv-${cssId(day)}`), hrvPts, BIO.ir);
    drawChart($(`#snd-resp-${cssId(day)}`), resp.map(r=>({t:tms(r.ts), v:r.value})), BIO.resp);
    drawChart($(`#snd-spo2-${cssId(day)}`), spo2.map(r=>({t:tms(r.ts), v:r.value})), BIO.red);
    // Movement = per-point L2 norm of the gravity-vector delta between consecutive samples.
    const movePts = [];
    for(let i=1;i<grav.length;i++){
      const dx=grav[i].x-grav[i-1].x, dy=grav[i].y-grav[i-1].y, dz=grav[i].z-grav[i-1].z;
      const m=Math.sqrt(dx*dx+dy*dy+dz*dz);
      if(Number.isFinite(m)) movePts.push({t:tms(grav[i].ts), v:m});
    }
    drawChart($(`#snd-move-${cssId(day)}`), movePts, BIO.gx, {fixedMin:0});
  }catch(e){ /* charts show their own "no data" state */ }
}

function fmtNightDay(day){ return new Date(day+'T12:00:00Z')
  .toLocaleDateString('en',{weekday:'short',month:'short',day:'numeric'}); }
// Day strings are YYYY-MM-DD — safe as CSS/DOM ids as-is, but keep a helper for clarity.
function cssId(day){ return day; }
function tsec(iso){ const ms=new Date(iso).getTime(); return Number.isFinite(ms)?Math.floor(ms/1000):NaN; }

// Build a flat ordered list of {start,end,stage} (epoch sec) for the latest day. Prefer the
// real per-session stages from /v1/sleep; the daily row only has aggregate minutes.
function collectStages(sleep, d){
  const segs = [];
  (sleep||[]).forEach(s=>{
    (s.stages||[]).forEach(seg=>{
      const st = (seg.start!=null) ? +seg.start : null;
      const en = (seg.end!=null) ? +seg.end : null;
      if(st!=null && en!=null && en>st && seg.stage) segs.push({start:st, end:en, stage:seg.stage});
    });
  });
  segs.sort((a,b)=>a.start-b.start);
  return segs;
}

// Hypnogram: horizontal band, each stage segment a colored slice along time. If we have no
// per-segment data, fall back to a stacked proportional bar from the daily deep/rem/light mins.
function drawHypnogram(cv, segs, d){
  const {x, cssW, cssH} = sizeCanvas(cv, 300);
  const H = cssH;
  if(segs.length){
    const t0 = segs[0].start, t1 = segs[segs.length-1].end, span=(t1-t0)||1;
    segs.forEach(s=>{
      const X = (s.start-t0)/span*cssW, W = Math.max(1,(s.end-s.start)/span*cssW);
      x.fillStyle = STAGE[s.stage] || CAT.unknown;
      x.fillRect(X, 0, W, H);
    });
  } else {
    // Stacked fallback from aggregate minutes (deep/rem/light/wake-as-disturbance).
    const parts = [
      ['deep',  numOrNull(d.deep_min)||0],
      ['rem',   numOrNull(d.rem_min)||0],
      ['light', numOrNull(d.light_min)||0],
    ];
    const tot = parts.reduce((a,[,v])=>a+v,0);
    if(tot<=0){ x.fillStyle='#3f5450'; x.font='11px "IBM Plex Mono"'; x.fillText('no stage data', 6, H/2+4); return; }
    let X=0;
    parts.forEach(([k,v])=>{ const W=v/tot*cssW; x.fillStyle=STAGE[k]||CAT.unknown; x.fillRect(X,0,Math.max(0,W),H); X+=W; });
  }
}

function renderStageLegend(el, segs, d){
  // Show per-stage total minutes. Real segments take priority; fall back to daily aggregate.
  const mins = {};
  segs.forEach(s=>{ mins[s.stage] = (mins[s.stage]||0) + (s.end-s.start)/60; });
  const order = STAGE_ORDER.filter(k=>mins[k]!=null);
  if(order.length){
    el.innerHTML = order.map(k=>`<span class="slg"><i style="background:${STAGE[k]}"></i>${k} ${Math.round(mins[k])}m</span>`).join('');
    return;
  }
  // Fallback: build legend from daily aggregate minutes so it mirrors the stacked bar.
  if(d){
    const agg = [
      ['deep',  numOrNull(d.deep_min)],
      ['rem',   numOrNull(d.rem_min)],
      ['light', numOrNull(d.light_min)],
      ['wake',  numOrNull(d.disturbances)],  // disturbances as a proxy for wake if no wake_min
    ].filter(([,v])=>v!=null && v>0);
    if(agg.length){
      el.innerHTML = agg.map(([k,v])=>`<span class="slg"><i style="background:${STAGE[k]}"></i>${k} ${Math.round(v)}m</span>`).join('');
      return;
    }
  }
  // No data at all: show bare stage swatches without minutes.
  el.innerHTML = STAGE_ORDER.map(k=>`<span class="slg"><i style="background:${STAGE[k]}"></i>${k}</span>`).join('');
}

// "asleep 2:41 AM · woke 12:01 PM" in the viewer's local tz, from the daily row's
// sleep_start/sleep_end (ISO). Empty string when the night has no detected sleep.
function bedwakeLine(d){
  if(!d || d.sleep_start==null || d.sleep_end==null) return '';
  return `asleep ${fmtClock(d.sleep_start)} · woke ${fmtClock(d.sleep_end)}`;
}

function disturbLine(d){
  const bits = [];
  if(d.deep_min!=null)  bits.push(`deep ${Math.round(d.deep_min)}m`);
  if(d.rem_min!=null)   bits.push(`rem ${Math.round(d.rem_min)}m`);
  if(d.light_min!=null) bits.push(`light ${Math.round(d.light_min)}m`);
  if(d.disturbances!=null) bits.push(`${d.disturbances} disturbance${d.disturbances===1?'':'s'}`);
  return bits.join(' · ') || 'no stage breakdown';
}

function numOrNull(v){ return v==null ? null : +v; }
function fmtMetric(v, unit){ return v==null ? '—' : `${Math.round(v)} ${unit}`; }
function fmtDur(min){ const total=Math.round(min); const h=Math.floor(total/60), m=total%60; return `${h}:${String(m).padStart(2,'0')}`; }
function dayMs(day){ return new Date(day+'T12:00:00Z').getTime(); }

function renderSummary(hr){
  $('#st-nhr').textContent = hr.length;
  if(hr.length){
    const v = hr.map(r=>r.bpm);
    const avg = Math.round(v.reduce((a,b)=>a+b,0)/v.length);
    $('#st-hr').textContent = `${avg} / ${Math.min(...v)} / ${Math.max(...v)}`;
  } else $('#st-hr').textContent = '—';
}

/* Size a canvas for crisp HiDPI rendering and return its 2D context + CSS dims.
   Why dataset.h: setting `cv.height` reflects into the height ATTRIBUTE, so reading
   `cv.getAttribute('height')` on a later redraw would read back the dpr-scaled buffer
   height and compound it (×dpr every reload → the persistent SIGNALS charts grew on
   each ⟳ reload, only resetting on a hard refresh). We cache the logical height in
   dataset.h (immutable) the first time we see the canvas, and pin the DISPLAY height
   via style so the scaled buffer never changes layout. */
function sizeCanvas(cv, fallbackW){
  const dpr = window.devicePixelRatio||1;
  const cssW = cv.clientWidth||cv.parentElement.clientWidth||fallbackW;
  if(cv.dataset.h==null) cv.dataset.h = (cv.getAttribute('height')||cv.clientHeight||100);
  const cssH = +cv.dataset.h;
  cv.width = cssW*dpr; cv.height = cssH*dpr;
  cv.style.height = cssH+'px';
  const x = cv.getContext('2d'); x.scale(dpr,dpr);
  x.clearRect(0,0,cssW,cssH);
  return {x, cssW, cssH};
}

/* Signals time-window: size the shared scroll container's canvases so 24h fills the
   visible width. A wider loaded range (the range buttons) makes the content wider than
   the viewport → horizontal scroll. Capped at 10 screens: a canvas wider than ~16k
   DEVICE px (≈ visW*10 × dpr) silently fails to paint in some browsers (Safari), so for
   very long ranges we compress to >24h/screen rather than break. Sets a CSS var the
   canvases read for their width (see style.css). */
const SIGNALS_VISIBLE_MS = 24 * 3600 * 1000;   // 24h fills the panel
function applySignalsWindow(dom){
  const sc = $('#signals-scroll'); if(!sc) return;
  const visW = sc.clientWidth || 700;
  const span = (dom && dom.tMin!=null && dom.tMax!=null) ? (dom.tMax - dom.tMin) : 0;
  const w = span > 0
    ? Math.min(visW * 10, Math.max(visW, Math.round(span / SIGNALS_VISIBLE_MS * visW)))
    : visW;
  sc.style.setProperty('--sig-w', w + 'px');
}
function scrollSignalsToNow(){
  const sc = $('#signals-scroll'); if(!sc) return;
  sc.scrollLeft = sc.scrollWidth;   // right edge = live "now"
}

// Contiguous [t0,t1] (ms) ranges where battery.charging is true → shaded bands on the chart.
function chargingBands(batt){
  const out=[]; let start=null, prev=null;
  for(const r of (batt||[])){
    const t=tms(r.ts);
    if(r.charging){ if(start==null) start=t; }
    else if(start!=null){ out.push([start, prev!=null?prev:t]); start=null; }
    prev=t;
  }
  if(start!=null && prev!=null) out.push([start, prev]);
  return out;
}

/* ---------- canvas line chart ---------- */
function drawChart(cv, pts, color, opts={}){
  const {x, cssW, cssH} = sizeCanvas(cv, 600);
  const padL=42, padR=10, padT=10, padB=18, W=cssW-padL-padR, H=cssH-padT-padB;
  // axes baseline
  x.strokeStyle='#1c2a30'; x.lineWidth=1;
  x.beginPath(); x.moveTo(padL,padT); x.lineTo(padL,padT+H); x.lineTo(padL+W,padT+H); x.stroke();
  const validPts = pts.filter(p => Number.isFinite(p.v));
  if(validPts.length===0){ x.fillStyle='#3f5450'; x.font='11px "IBM Plex Mono"';
    x.fillText('no data in window', padL+10, padT+H/2); return; }
  const ts=validPts.map(p=>p.t), vs=validPts.map(p=>p.v);
  let mn = opts.fixedMin!=null?opts.fixedMin:Math.min(...vs);
  let mx = opts.fixedMax!=null?opts.fixedMax:Math.max(...vs);
  if(mn===mx){ mn-=1; mx+=1; }
  // x-domain: a SHARED [tMin,tMax] (opts) makes all stream charts line up on one time axis so
  // they're directly comparable (a stream that doesn't cover the whole span shows gaps at the
  // ends — the honest representation). Falls back to this chart's own extent when unset.
  const t0 = opts.tMin!=null?opts.tMin:Math.min(...ts);
  const t1 = opts.tMax!=null?opts.tMax:Math.max(...ts);
  const tspan=(t1-t0)||1;
  const px=t=>padL+(t-t0)/tspan*W, py=v=>padT+H-(v-mn)/(mx-mn)*H;
  // Shaded time bands (e.g. battery charging stretches): soft fill behind the trace.
  if(opts.bands && opts.bands.length){
    x.fillStyle='rgba(122,162,255,0.13)';
    for(const [b0,b1] of opts.bands){
      const X0=Math.max(padL, px(b0)), X1=Math.min(padL+W, px(b1));
      if(X1>X0) x.fillRect(X0, padT, X1-X0, H);
    }
  }
  // y gridlines + labels. Charts inside the signals scroll get a sticky y-axis overlay
  // (renderYAxis), so skip the on-canvas value text there to avoid double labels.
  const stickyY = !!cv.closest('.signals-scroll');
  x.fillStyle='#6f8a86'; x.font='9px "IBM Plex Mono"'; x.textAlign='right';
  for(let i=0;i<=2;i++){ const v=mn+(mx-mn)*i/2, yy=py(v);
    x.strokeStyle='#101a1f'; x.beginPath(); x.moveTo(padL,yy); x.lineTo(padL+W,yy); x.stroke();
    if(!stickyY) x.fillText(Math.round(v), padL-6, yy+3); }
  // line + glow — break the path across data gaps so a missing window doesn't render as a
  // misleading diagonal. (See gapThreshold + each contiguous run drawn as its own subpath.)
  const gap = gapThreshold(ts, opts);
  x.beginPath();
  validPts.forEach((p,i)=>{ const X=px(p.t),Y=py(p.v);
    (i && (p.t - validPts[i-1].t) <= gap) ? x.lineTo(X,Y) : x.moveTo(X,Y); });
  x.strokeStyle=color; x.lineWidth=1.6; x.shadowColor=color; x.shadowBlur=7; x.stroke(); x.shadowBlur=0;
  // last-point dot
  const last=validPts[validPts.length-1];
  x.fillStyle=color; x.beginPath(); x.arc(px(last.t),py(last.v),2.4,0,7); x.fill();
  drawXTicks(x, t0, t1, px, padL, padT, W, H);
  if(opts.nowEdge){   // right edge = live wall-clock now; the gap from the last point to it = sync lag
    x.strokeStyle='#2a4a3f'; x.beginPath(); x.moveTo(padL+W,padT); x.lineTo(padL+W,padT+H); x.stroke();
    x.fillStyle='#5fe3a1'; x.textAlign='right'; x.font='9px "IBM Plex Mono"';
    x.fillText('now', padL+W, padT+H+13);
  }
  cv._hover = {t0, t1, padL, W, series:[{pts:validPts, color, name:_chartName(cv), unit:_chartUnit(cv)}]};
  bindChartHover(cv);
  renderYAxis(cv, mn, mx, fmtAxis);
}

/* ---------- multi-series canvas line chart (shares axes + scaling with drawChart) ---------- */
function drawMultiChart(cv, series, opts={}){
  const {x, cssW, cssH} = sizeCanvas(cv, 600);
  const padL=42, padR=10, padT=10, padB=18, W=cssW-padL-padR, H=cssH-padT-padB;
  x.strokeStyle='#1c2a30'; x.lineWidth=1;
  x.beginPath(); x.moveTo(padL,padT); x.lineTo(padL,padT+H); x.lineTo(padL+W,padT+H); x.stroke();
  const allPts = series.flatMap(s=>s.pts);
  if(allPts.length===0){ x.fillStyle='#3f5450'; x.font='11px "IBM Plex Mono"';
    x.fillText('no data in window', padL+10, padT+H/2); return; }
  const ts=allPts.map(p=>p.t), vs=allPts.map(p=>p.v);
  let mn = opts.fixedMin!=null?opts.fixedMin:Math.min(...vs);
  let mx = opts.fixedMax!=null?opts.fixedMax:Math.max(...vs);
  if(mn===mx){ mn-=1; mx+=1; }
  // Shared x-domain (opts.tMin/tMax) — see drawChart. Falls back to own extent when unset.
  const t0 = opts.tMin!=null?opts.tMin:Math.min(...ts);
  const t1 = opts.tMax!=null?opts.tMax:Math.max(...ts);
  const tspan=(t1-t0)||1;
  const px=t=>padL+(t-t0)/tspan*W, py=v=>padT+H-(v-mn)/(mx-mn)*H;
  // y gridlines + labels (shared). Skip on-canvas text for signals charts (sticky overlay).
  const stickyY = !!cv.closest('.signals-scroll');
  x.fillStyle='#6f8a86'; x.font='9px "IBM Plex Mono"'; x.textAlign='right';
  for(let i=0;i<=2;i++){ const v=mn+(mx-mn)*i/2, yy=py(v);
    x.strokeStyle='#101a1f'; x.beginPath(); x.moveTo(padL,yy); x.lineTo(padL+W,yy); x.stroke();
    if(!stickyY) x.fillText(fmtAxis(v), padL-6, yy+3); }
  // one glowing line per series — each breaks across its own data gaps (per-series spacing).
  series.forEach(s=>{
    if(!s.pts.length) return;
    const sp = s.pts.filter(p=>Number.isFinite(p.v));
    if(!sp.length) return;
    const gap = gapThreshold(sp.map(p=>p.t), opts);
    x.beginPath();
    sp.forEach((p,i)=>{ const X=px(p.t),Y=py(p.v);
      (i && (p.t - sp[i-1].t) <= gap) ? x.lineTo(X,Y) : x.moveTo(X,Y); });
    x.strokeStyle=s.color; x.lineWidth=1.6; x.shadowColor=s.color; x.shadowBlur=7; x.stroke(); x.shadowBlur=0;
    const last=sp[sp.length-1];
    x.fillStyle=s.color; x.beginPath(); x.arc(px(last.t),py(last.v),2.4,0,7); x.fill();
  });
  drawXTicks(x, t0, t1, px, padL, padT, W, H);
  if(opts.nowEdge){   // right edge = live wall-clock now; the gap from the last point to it = sync lag
    x.strokeStyle='#2a4a3f'; x.beginPath(); x.moveTo(padL+W,padT); x.lineTo(padL+W,padT+H); x.stroke();
    x.fillStyle='#5fe3a1'; x.textAlign='right'; x.font='9px "IBM Plex Mono"';
    x.fillText('now', padL+W, padT+H+13);
  }
  cv._hover = {t0, t1, padL, W, series: series.map((s,i)=>({
    pts: s.pts.filter(p=>Number.isFinite(p.v)), color: s.color,
    name: s.name || ['x','y','z'][i] || ('s'+i), unit:_chartUnit(cv)}))};
  bindChartHover(cv);
  renderYAxis(cv, mn, mx, fmtAxis);
}
function fmtAxis(v){ return Math.abs(v) < 10 && !Number.isInteger(v) ? v.toFixed(2) : Math.round(v); }

/* ---------- chart hover tooltip + sticky y-axis (signals scroll) ---------- */
// Chart name / unit come from the sibling .chart-label so we don't thread them through draws.
function _chartName(cv){ const w=cv.closest('.chart-wrap'); const s=w&&w.querySelector('.chart-label>span'); return s?s.textContent.trim():''; }
function _chartUnit(cv){ const w=cv.closest('.chart-wrap'); const u=w&&w.querySelector('.chart-unit'); return u&&u.firstChild?u.firstChild.textContent.trim():''; }
function _nearestByT(pts, t){
  if(!pts||!pts.length) return null;
  let best=pts[0], bd=Math.abs(pts[0].t-t);
  for(const p of pts){ const d=Math.abs(p.t-t); if(d<bd){ bd=d; best=p; } }
  return {p:best, dt:bd};
}
function bindChartHover(cv){
  if(cv._hoverBound) return; cv._hoverBound=true;
  cv.addEventListener('mousemove', onChartHover);
  cv.addEventListener('mouseleave', hideChartTip);
}
function hideChartTip(){ $('#tooltip').classList.remove('show'); }
function onChartHover(e){
  const cv=e.currentTarget, h=cv._hover; if(!h){ hideChartTip(); return; }
  const x=e.offsetX;
  if(x < h.padL || x > h.padL + h.W){ hideChartTip(); return; }
  const t = h.t0 + (x - h.padL)/h.W*(h.t1 - h.t0);
  const tol = Math.max(60000, (h.t1 - h.t0)/h.W * 6);   // within ~6px of a sample
  const rows=[];
  for(const s of h.series){
    const n=_nearestByT(s.pts, t);
    if(n && n.dt <= tol) rows.push({name:s.name, color:s.color, v:n.p.v, unit:s.unit});
  }
  const tip=$('#tooltip');
  if(!rows.length){ hideChartTip(); return; }
  tip.innerHTML = `<div class="tt-name">${fmtTime(t)}</div>` + rows.map(r=>
    `<div class="tt-val"><i style="background:${r.color}"></i>${r.name?r.name+' ':''}<b>${fmtAxis(r.v)}</b> ${r.unit||''}</div>`).join('');
  tip.style.left = Math.min(e.clientX+14, innerWidth-220) + 'px';
  tip.style.top  = (e.clientY+16) + 'px';
  tip.classList.add('show');
}
// Frozen y-axis: write min/mid/max value labels into a sticky overlay over the plot's
// left edge (only for charts inside the signals scroll container).
function renderYAxis(cv, mn, mx, fmt){
  const wrap = cv.closest('.chart-wrap');
  if(!wrap || !wrap.closest('.signals-scroll')) return;
  let g = wrap.querySelector('.yaxis');
  if(!g){ g=document.createElement('div'); g.className='yaxis'; wrap.insertBefore(g, wrap.firstChild); }
  const cssH=+cv.dataset.h||100, padT=10, padB=18, H=cssH-padT-padB;
  const base = cv.offsetTop - g.offsetTop;   // canvas top relative to the sticky overlay
  const mk=(v,yy)=>`<span style="top:${(base+yy).toFixed(1)}px">${fmt(v)}</span>`;
  g.innerHTML = mk(mx, padT) + mk((mn+mx)/2, padT+H/2) + mk(mn, padT+H);
}

/* x-axis time ticks: ~5-8 evenly spaced at a "nice" interval (so it's easy to read WHEN data is
   from). Faint vertical gridline + tick + label per tick; the date ("Mon d") is shown on the first
   tick and whenever the calendar day changes, otherwise just HH:MM — so multi-day spans aren't
   ambiguous. t0/t1 are ms (chart timestamps come from tms()/Date.getTime()). */
const _NICE_STEPS_MS = [60e3, 2*60e3, 5*60e3, 10*60e3, 15*60e3, 30*60e3,
                        3600e3, 2*3600e3, 3*3600e3, 6*3600e3, 12*3600e3, 24*3600e3];
function drawXTicks(x, t0, t1, px, padL, padT, W, H){
  const span = (t1 - t0) || 1;
  const target = span / 7;                                  // aim for ~6-8 ticks
  const step = _NICE_STEPS_MS.find(s => s >= target) || _NICE_STEPS_MS[_NICE_STEPS_MS.length-1];
  x.font = '9px "IBM Plex Mono"';
  let prevDay = null;
  for(let t = Math.ceil(t0/step)*step; t <= t1 + 1; t += step){
    const X = px(t);
    if(X < padL - 1 || X > padL + W + 1) continue;
    x.strokeStyle = '#101a1f'; x.beginPath(); x.moveTo(X, padT); x.lineTo(X, padT+H); x.stroke();   // gridline
    x.strokeStyle = '#1c2a30'; x.beginPath(); x.moveTo(X, padT+H); x.lineTo(X, padT+H+3); x.stroke(); // tick
    const d = new Date(t);
    const day = d.toLocaleDateString('en', {month:'short', day:'numeric'});
    const hm  = d.toLocaleTimeString('en', {hour:'numeric', minute:'2-digit'});
    const label = (day !== prevDay) ? `${day} ${hm}` : hm;
    prevDay = day;
    x.fillStyle = '#6f8a86';
    x.textAlign = X < padL + 26 ? 'left' : (X > padL + W - 26 ? 'right' : 'center');
    x.fillText(label, X, padT + H + 13);
  }
}

/* Gap-break threshold (in ms — chart timestamps come from tms()/Date.getTime()). Two
   consecutive points farther apart than this are NOT connected, so a missing window reads
   as "no data here" instead of a fake interpolated diagonal. Threshold = max(120s, 6×median
   sample spacing) so it adapts to coarser series (e.g. daily) without breaking ~1Hz streams.
   opts.gapMs overrides explicitly. Exported on globalThis for Node tests. */
function gapThreshold(ts, opts={}){
  if(opts.gapMs!=null) return opts.gapMs;
  const FLOOR = 120000; // 120 s
  if(!ts || ts.length < 3) return FLOOR;
  const sorted = ts.slice().sort((a,b)=>a-b);
  const diffs = [];
  for(let i=1;i<sorted.length;i++){ const d=sorted[i]-sorted[i-1]; if(d>0) diffs.push(d); }
  if(!diffs.length) return FLOOR;
  diffs.sort((a,b)=>a-b);
  const median = diffs[Math.floor(diffs.length/2)];
  return Math.max(FLOOR, median*6);
}
if(typeof globalThis!=='undefined') globalThis.gapThreshold = gapThreshold;

/* Shared x-axis time domain for the raw-stream panels. Takes an array of point-arrays
   ([{t,v},…]) and returns {tMin,tMax} = the union extent across every series that has at
   least one finite-t point. Returns {} (no override) when nothing has data, so charts fall
   back to their own extent. Threaded into every stream chart's opts so they all use the SAME
   x-scale and line up vertically — see loadAll(). Exported for Node tests. */
function streamDomain(series){
  let tMin=Infinity, tMax=-Infinity;
  for(const pts of (series||[])){
    for(const p of (pts||[])){
      if(p && Number.isFinite(p.t)){ if(p.t<tMin) tMin=p.t; if(p.t>tMax) tMax=p.t; }
    }
  }
  if(tMin===Infinity || tMax===-Infinity) return {};
  return {tMin, tMax};
}
if(typeof globalThis!=='undefined') globalThis.streamDomain = streamDomain;

/* ---------- events ---------- */
function renderEvents(events){
  $('#ev-meta').textContent = `${events.length}`;
  if(!events.length){ $('#events').innerHTML = `<div class="empty">no events in window</div>`; return; }
  $('#events').innerHTML = events.map(e=>{
    const kind=(e.kind||'').replace(/\(\d+\)$/,'');
    return `<div class="erow"><span class="et">${fmtFull(e.ts)}</span>`+
      `<span class="ek" style="color:${CAT.event}">${kind}</span></div>`;
  }).join('');
}

/* ---------- batches + frames ---------- */
function renderBatches(batches){
  $('#batch-meta').textContent = `${batches.length}`;
  $('#st-nbatch').textContent = batches.length;
  if(!batches.length){
    $('#batch-list').innerHTML = `<div class="empty">no batches</div>`;
    $('#frame-list').innerHTML = `<div class="empty">no batches to load</div>`;
    $('#inspector').hidden = true;
    return;
  }
  $('#batch-list').innerHTML = batches.map(b=>{
    const span = b.start_ts ? `${fmtFull(b.start_ts)}` : 'no decoded ts';
    const kb = (b.byte_size/1024).toFixed(1);
    return `<div class="brow" data-id="${b.batch_id}">`+
      `<span class="bt">${span}</span>`+
      `<span class="bp">${b.packet_count} pkt</span>`+
      `<span class="bz">${kb} KB</span></div>`;
  }).join('');
  document.querySelectorAll('.brow').forEach(r => r.onclick = () => {
    document.querySelectorAll('.brow').forEach(x=>x.classList.remove('sel'));
    r.classList.add('sel'); loadFrames(r.dataset.id);
  });
  // auto-open the most recent batch so the view lands fully populated
  const first = document.querySelector('.brow');
  if(first){ first.classList.add('sel'); loadFrames(first.dataset.id); }
}

async function loadFrames(batchId){
  $('#frame-meta').textContent = 'loading…';
  try{
    const frames = await api(`/v1/batches/${batchId}/frames`);
    $('#frame-meta').textContent = `${frames.length}`;
    if(!frames.length){ $('#frame-list').innerHTML = `<div class="empty">no frames</div>`; return; }
    $('#frame-list').innerHTML = frames.map((f,i)=>{
      const crc = f.crc_ok===false?'✗':(f.crc_ok?'✓':'—');
      return `<div class="frrow" data-i="${i}"><span class="frseq">#${f.seq}</span>`+
        `<span class="frtype" style="color:${typeColor(f.type_name)}">${f.type_name||'?'}</span>`+
        `<span class="frcrc ${f.crc_ok===false?'bad':''}">${crc}</span></div>`;
    }).join('');
    document.querySelectorAll('.frrow').forEach(r => r.onclick = () => {
      document.querySelectorAll('.frrow').forEach(x=>x.classList.remove('sel'));
      r.classList.add('sel'); state.selField=-1; inspectFrame(frames[+r.dataset.i]);
    });
    // auto-inspect the first frame so the hex view lands populated
    const first = document.querySelector('.frrow');
    if(first){ first.classList.add('sel'); state.selField=-1; inspectFrame(frames[0], false); }
  }catch(e){ $('#frame-meta').textContent = 'error'; }
}

/* ---------- hex inspector (carried over from the live dashboard) ---------- */
function inspectFrame(p, scroll=true){
  state.frame = p;
  $('#inspector').hidden = false;
  const raw = hexToBytes(p.hex);
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
  $('#pkt-meta').textContent = `${p.type_name} · ${raw.length}B · crc ${p.crc_ok===false?'✗':(p.crc_ok?'✓':'—')}`;
  renderFields(p);
  applyFieldSel();
  if(scroll) $('#inspector').scrollIntoView({behavior:'smooth', block:'nearest'});
}
function applyFieldSel(){
  document.querySelectorAll('.byte.field-sel').forEach(b=>b.classList.remove('field-sel','hl'));
  if(state.selField<0) return;
  document.querySelectorAll(`.byte[data-field="${state.selField}"]`).forEach(b=>b.classList.add('field-sel','hl'));
}
const tip=$('#tooltip');
$('#hexgrid').addEventListener('mouseover',e=>{
  const b=e.target.closest('.byte'); if(!b||b.dataset.field===undefined||!state.frame) return;
  const fi=+b.dataset.field; if(fi<0) return;
  const f=state.frame.fields[fi];
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
  if(fi!==state.selField) document.querySelectorAll(`.byte[data-field="${fi}"]`).forEach(x=>x.classList.remove('hl'));
  tip.classList.remove('show'); document.querySelectorAll('.frow.sel').forEach(r=>r.classList.remove('sel'));
});
function renderFields(p){
  const f=p.fields||[];
  $('#fields').innerHTML = f.map((x,i)=>{
    const col=CAT[x.cat]||CAT.unknown;
    return `<div class="frow" data-i="${i}"><span class="swatch" style="background:${col}"></span>`+
      `<span class="fname">${x.name}</span><span class="fval" title="${fmtVal(x.value)}">${fmtVal(x.value)}</span>`+
      (x.note?`<span class="fnote">${x.note}</span>`:'')+`</div>`;
  }).join('');
  document.querySelectorAll('#fields .frow').forEach(row=>{
    row.onmouseenter=()=>{ const i=+row.dataset.i; document.querySelectorAll(`.byte[data-field="${i}"]`).forEach(b=>b.classList.add('hl')); };
    row.onmouseleave=()=>{ const i=+row.dataset.i; if(i!==state.selField) document.querySelectorAll(`.byte[data-field="${i}"]`).forEach(b=>b.classList.remove('hl')); };
    row.onclick=()=>{ state.selField=+row.dataset.i; applyFieldSel(); };
  });
  const pv=p.parsed||{};
  $('#parsed').innerHTML = Object.keys(pv).length? Object.entries(pv).map(([k,v])=>
    `<div class="kv"><b>${k}</b><span>${fmtVal(v)}</span></div>`).join('') :
    `<div class="kv"><b>no parsed fields</b><span>raw only</span></div>`;
}
function renderLegend(cats){
  $('#legend').innerHTML = cats.map(c=>`<span class="lg"><i style="background:${CAT[c]||CAT.unknown}"></i>${c}</span>`).join('');
}

/* ---------- utils ---------- */
function typeColor(t){return ({REALTIME_DATA:CAT.hr,REALTIME_RAW_DATA:CAT.accel,HISTORICAL_DATA:CAT.meta,
  EVENT:CAT.event,COMMAND_RESPONSE:CAT.cmd,METADATA:CAT.meta,CONSOLE_LOGS:CAT.text})[t]||CAT.unknown;}
function apiKey(){
  let k = localStorage.getItem('whoop_key');
  if(!k){ k = prompt('Whoop API key (read access):') || ''; if(k) localStorage.setItem('whoop_key', k); }
  return k;
}
async function api(path){
  const r = await fetch(path, { headers: { 'Authorization': 'Bearer ' + apiKey() } });
  if(r.status === 401){ localStorage.removeItem('whoop_key'); throw new Error('401 unauthorized — refresh to re-enter the key'); }
  if(!r.ok) throw new Error(`${r.status} ${path}`);
  return r.json();
}
async function apiPost(path, body){
  const r = await fetch(path, { method:'POST', headers:{ 'Authorization':'Bearer '+apiKey(), 'Content-Type':'application/json' }, body: JSON.stringify(body) });
  if(r.status === 401){ localStorage.removeItem('whoop_key'); throw new Error('401 unauthorized'); }
  if(!r.ok) throw new Error(`${r.status} ${path}`);
  return r.json();
}
function status(m){ $('#status').textContent = m; }
function tms(iso){ return new Date(iso).getTime(); }
function hexToBytes(h){ const a=new Uint8Array(h.length/2); for(let i=0;i<a.length;i++)a[i]=parseInt(h.substr(i*2,2),16); return a; }
function fmtVal(v){ if(v==null)return '—'; if(Array.isArray(v))return '['+v.join(', ')+']'; if(typeof v==='object')return JSON.stringify(v); return String(v); }
function fmtTime(ms){ return new Date(ms).toLocaleString('en',{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'}); }
function fmtFull(iso){ return new Date(iso).toLocaleString('en',{year:'2-digit',month:'short',day:'numeric',hour:'numeric',minute:'2-digit',second:'2-digit'}); }
// Clock time (e.g. "2:41 AM") in the VIEWER's local timezone — used for bed/wake times.
// `t` is an ISO string or epoch ms; both parse via Date. Omitting hour12 lets the
// locale decide, but 'en' defaults to 12-hour with AM/PM.
function fmtClock(t){ return new Date(t).toLocaleTimeString('en',{hour:'numeric',minute:'2-digit'}); }
function labelRange(s){ s=+s; if(s>=2592000)return '30d'; if(s>=604800)return '7d'; if(s>=86400)return '24h'; return s+'s'; }
window.addEventListener('resize', ()=>{ if(state.device) loadAll(); });
