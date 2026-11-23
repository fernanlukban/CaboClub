const root = document.querySelector("#club");
const connection = document.querySelector("#connection");
const query = new URLSearchParams(location.search);
const isIPad = /iPad/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const mode = query.has("board") || (isIPad && !query.has("room") && !query.has("play")) ? "board" : "player";
const key = `cabo-session-${mode}`;
let session;
try { session = JSON.parse(localStorage.getItem(key) || "null"); } catch { session = null; }
if (query.get("room") && session?.room !== query.get("room").toUpperCase()) session = null;
let snapshot;
let selection = [];
let target;
let busy = false;
let polling = false;
let page = query.has("room") ? "join" : "home";
let notice = "";
let pairing = "";
let insertLeft = false;
let tableControlsOpen = false;
let moveAnimation;

const labels = {
  dealRound:"Deal cards",dealNextRound:"Deal next round",dealNewGame:"New game",
  cancelSetup:"Cancel deal",revealInitialCards:"Reveal 2",confirmInitialLook:"I’ve looked",
  beginPlay:"Start game",drawDeck:"Draw from deck",drawDiscard:"Take discard",
  discardDrawn:"Discard drawn card",discardHandCard:"Discard selected card",callMatch:"Call match",
  callCabo:"Call Cabo",useDrawnPower:"Use card power",peek:"Peek",spy:"Spy",swap:"Swap",
  pairBoard:"Pair a board",revokeBoard:"Revoke board access",
};
const errors = {
  staleRevision:"The table changed. Choose your move again.",staleDeal:"This deal has changed.",
  invalidName:"Enter a name with 1 to 22 characters.",roomUnavailable:"This room is full or already playing.",
  invalidPairing:"That board pairing code has expired or is incorrect.",roomNotFound:"This room has expired or does not exist.",
  invalidSession:"Your session ended. Join again to continue.",actionUnavailable:"That move is unavailable now.",
  roomCapacity:"The server is full. Try creating a table later.",
};

function node(tag, props = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") element.className = value;
    else if (key === "click") element.addEventListener("click", value);
    else if (key === "submit") element.addEventListener("submit", value);
    else if (key === "disabled") element.disabled = value;
    else element.setAttribute(key, value);
  }
  element.append(...children.filter(child => child !== null && child !== undefined));
  return element;
}
function button(label, click, primary = false, disabled = false) {
  return node("button", { type:"button", class:primary?"primary":"", click, disabled }, label);
}
function field(label, id, value = "", max = 22) {
  return node("label", {}, label, node("input", { name:id, value, maxlength:max, required:"", autocomplete:"off" }));
}
function save(next) {
  session = next;
  if (next) localStorage.setItem(key, JSON.stringify(next));
  else localStorage.removeItem(key);
}
function endpoint(suffix = "") { return `/api/rooms/${encodeURIComponent(session.room)}${suffix}`; }
async function request(url, payload, credentials = session) {
  const headers = {};
  if (payload) headers["Content-Type"] = "application/json";
  if (credentials) headers.Authorization = `Bearer ${credentials.token}`;
  const response = await fetch(url, {
    method:payload?"POST":"GET", headers, body:payload?JSON.stringify(payload):undefined, cache:"no-store",
  });
  const data = await response.json();
  return [response, data];
}
function accept(next, force = false, renderNow = true) {
  if (!session || next.roomCode !== session.room) return;
  if (!force && snapshot && next.roomId === snapshot.roomId && next.revision <= snapshot.revision) return;
  if (snapshot && snapshot.dealId !== next.dealId) { selection = []; target = undefined; }
  const currentTargets = new Set(next.availableActions.flatMap(action => action.targets));
  const allowed = selectable(next);
  selection = selection.filter(slot => allowed.has(slot));
  if (target !== undefined && !currentTargets.has(target) && !allowed.has(target)) target = undefined;
  snapshot = next;
  if (renderNow) render();
}
function selectable(state = snapshot) {
  return new Set(state?.availableActions.flatMap(action => action.slots) ?? []);
}
async function connect(url, payload) {
  if (busy) return;
  busy = true;
  try {
    const [response, data] = await request(url, payload, null);
    if (!response.ok) { notice = errors[data.error] || "Could not join the table."; return; }
    save({ room:data.snapshot.roomCode, token:data.token });
    snapshot = undefined;
    const pageUrl = new URL(location.href);
    pageUrl.searchParams.set("room", session.room);
    history.replaceState(null, "", pageUrl);
    notice = "";
    accept(data.snapshot, true);
  } catch { notice = "The table is offline. Try again."; }
  finally { busy = false; render(); }
}
async function poll() {
  if (!session || busy || polling) return;
  polling = true;
  const active = session;
  try {
    const [response, data] = await request(endpoint(), undefined, active);
    if (session !== active || busy) return;
    if (response.ok) {
      connection.hidden = true;
      accept(data);
    } else if ([401,404].includes(response.status)) {
      leave(errors[data.error]);
    }
  } catch {
    if (session === active) { connection.textContent = "Reconnecting…"; connection.hidden = false; }
  } finally { polling = false; }
}
async function submit(choice) {
  if (!choice || busy || !snapshot) return;
  const active = session;
  const slots = choice.action === "swap" ? [selection[0], target] : choice.action === "spy" ? [target] : choice.max ? selection.slice() : [];
  if (slots.length < choice.min || slots.length > choice.max || slots.includes(undefined)) return;
  busy = true;
  render();
  const data = {
    commandId:globalThis.crypto?.randomUUID?.() || Date.now().toString(36) + Math.random().toString(36),
    expectedRevision:snapshot.revision, action:choice.action, slots, insertLeft, dealId:snapshot.dealId,
  };
  try {
    let result;
    try { result = await request(endpoint("/commands"), data, active); }
    catch { result = await request(endpoint("/commands"), data, active); }
    if (session !== active) return;
    const [response, body] = result;
    if ([401,404].includes(response.status)) { leave(errors[body.error]); return; }
    if (response.ok) {
      notice = "";
      if (choice.action === "pairBoard") { pairing = body.detail; tableControlsOpen = true; }
      if (["drawDeck","drawDiscard","discardHandCard","discardDrawn","callMatch"].includes(choice.action))
        moveAnimation = { action:choice.action, slots };
      if (choice.action === "revokeBoard") pairing = "";
      selection = [];
      target = undefined;
    } else notice = errors[body.detail] || "That action could not be applied.";
    if (body.snapshot) accept(body.snapshot, false, false);
  } catch { notice = "Connection lost. Check the table before trying again."; }
  finally { busy = false; render(); }
}
function leave(message = "") {
  save(null); snapshot = undefined; selection = []; target = undefined; pairing = ""; tableControlsOpen = false; notice = message;
  page = "home";
  const url = new URL(location.href);
  url.searchParams.delete("room");
  history.replaceState(null, "", url);
  render();
}
function navigate(next) { page = next; notice = ""; render(); }

function home() {
  const brand = node("h1",{}, "Cabo Club");
  if (session) return node("section",{class:"home"},brand,node("p",{},"Restoring your table…"), button("Back to home",()=>leave()));
  if (page === "home") {
    return node("section",{class:"home"},brand,node("p",{},mode === "board"?"The shared table":"Private tables for two to four players"),
      node("div",{class:"home-nav"},button("Join a game",()=>navigate("join")),button("Create a new game",()=>mode==="board"?connect("/api/rooms",{board:true}):navigate("create"),true)),
      node("a",{href:mode === "board"?"?play":"?board"},mode==="board"?"Play on this device":"Open tabletop mode"),notice?node("p",{role:"status"},notice):null);
  }
  const creating = page === "create";
  const form = node("form",{submit:event=>{
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const room = String(data.get("room") ?? "").trim().toUpperCase();
    if (creating) connect("/api/rooms",{board:false,name:data.get("name")});
    else if (mode === "board") connect(`/api/rooms/${encodeURIComponent(room)}/board`,{code:data.get("code")});
    else connect(`/api/rooms/${encodeURIComponent(room)}/join`,{name:data.get("name")});
  }},creating?null:field("Room code","room",query.get("room") || "",8),
    mode==="board"?field("Board pairing code","code","",10):field("Your name","name"),
    node("button",{type:"submit",class:"primary",disabled:busy},creating?"Create room":"Join room"));
  return node("section",{class:"home"},brand,node("h2",{},creating?"Create a game":"Join a game"),form,
    notice?node("p",{role:"status"},notice):null,button("Back",()=>navigate("home")));
}
function top(showTableControls = false) {
  return node("header",{class:"top"},node("span",{class:"brand"},"Cabo Club"),
    node("span",{},`${snapshot.roomCode} · ${snapshot.round ? "Round " + snapshot.round : "Lobby"}`),
    node("div",{class:"top-controls"},showTableControls?button("Table",()=>{tableControlsOpen=!tableControlsOpen;render();}):null,button("Leave",()=>leave())));
}
function card(view, index = 0, action, selected = false, name) {
  const rank = view?.rank;
  const face = rank !== null && rank !== undefined;
  return node("button",{
    type:"button",class:`card ${face?"face":""} ${selected?"selected":""} ${view?"":"empty"}`,
    "aria-label": name || `Card ${index+1}${face?", rank "+rank:", face down"}`,
    "aria-pressed":String(selected),disabled:!action || busy,click:action ?? (()=>{}),
    style:`--index:${index}`,
    "data-sprite":view?.slot === undefined ? (name ?? `card-${index}`) : String(view.slot),
  },node("span",{class:"card-inner"},view?(face?String(rank):"✦"):"—"));
}
function piles(interactive = false) {
  const draw = snapshot.availableActions.find(x=>x.action==="drawDeck");
  const discard = snapshot.availableActions.find(x=>x.action==="drawDiscard");
  const deckCard = card(snapshot.deckCount?{rank:null}:null,0,interactive&&draw?()=>submit(draw):null,false,"Draw from deck");
  if (interactive && draw) {
    let start;
    deckCard.addEventListener("pointerdown", event => { start = [event.clientX,event.clientY]; deckCard.setPointerCapture(event.pointerId); });
    deckCard.addEventListener("pointerup", event => {
      if (start && Math.hypot(event.clientX-start[0],event.clientY-start[1]) > 25) submit(draw);
      start = undefined;
    });
    deckCard.addEventListener("pointercancel", () => { start = undefined; });
  }
  return node("div",{class:"piles"},
    node("div",{class:"pile"},deckCard,`Deck · ${snapshot.deckCount}`),
    node("div",{class:"pile"},card(snapshot.discardTop===null?null:{rank:snapshot.discardTop},0,
      interactive&&discard?()=>submit(discard):null,false,`Take discard${snapshot.discardTop===null?"":", rank "+snapshot.discardTop}`),"Discard"));
}
function status() {
  const turn = snapshot.players.find(p=>p.seatId===snapshot.turnSeatId);
  if (notice) return notice;
  if (snapshot.phase==="initialPreview") return "Waiting for opening looks";
  if (snapshot.phase==="readyToStart") return "Everyone is ready";
  if (turn) return turn.seatId===snapshot.youSeatId?"Your turn":`${turn.name}’s turn`;
  return snapshot.message;
}
function toggle(slot) {
  if (selection.includes(slot)) selection = selection.filter(id=>id!==slot);
  else {
    const limit = Math.max(...snapshot.availableActions.filter(action=>action.slots.includes(slot)&&action.action!=="spy").map(action=>action.action==="swap"?1:action.max),1);
    selection = limit===1 ? [slot] : selection.length<limit ? [...selection,slot] : selection;
  }
  render();
}
function actionStrip(filter = () => true) {
  const visible = snapshot.availableActions.filter(filter);
  return node("div",{class:"actions"},...visible.map(choice=>{
    let label = labels[choice.action];
    if (choice.action==="beginPlay" && snapshot.round>1) label="Start round";
    const count = choice.action==="swap" ? selection.length + Number(target!==undefined) :
      choice.action==="spy" ? Number(target!==undefined) : selection.length;
    const valid = !choice.max || (count>=choice.min && count<=choice.max &&
      (choice.action==="spy" || selection.every(id=>choice.slots.includes(id))) &&
      (choice.action!=="swap" || choice.targets.includes(target)));
    return button(label,()=>submit(choice),choice.action!=="pairBoard"&&choice.action!=="revokeBoard",busy||!valid);
  }),visible.some(action=>action.chooseSide)&&selection.length>=2
    ?button(insertLeft?"Keep failed draw on left":"Keep failed draw on right",()=>{insertLeft=!insertLeft;render();}):null);
}
function summary(showCards = false) {
  return node("div",{class:"seats"},...snapshot.players.map(p=>{
    const result = snapshot.results.find(item=>item.seatId===p.seatId);
    return node("div",{class:`seat ${p.seatId===snapshot.turnSeatId?"current":""}`},
      node("strong",{},p.name),node("small",{},["initialPreview","readyToStart"].includes(snapshot.phase)?(p.ready?"Ready":"Looking"): `${p.score} points${result?` · +${result.points} this round`:""}`),
      showCards?node("div",{class:"result-cards"},...p.cards.map((item,index)=>card(item,index))):null);
  }));
}
function lobbyOrBoard() {
  const lobby = snapshot.phase==="lobby";
  const finished = snapshot.phase==="gameOver";
  const results = snapshot.phase==="roundResults" || finished;
  const heading = lobby?"Your table":finished?"Game over":results?"Round results":
    snapshot.phase==="readyToStart"?"Ready to play":snapshot.phase==="initialPreview"?"Opening look":status();
  const joinUrl = new URL(location.pathname,location.origin);
  joinUrl.searchParams.set("room",snapshot.roomCode);
  const invite = lobby?node("div",{class:"invite"},
    node("img",{src:`/api/qr?url=${encodeURIComponent(joinUrl.href)}`,alt:"Scan to join the Cabo table"}),
    node("span",{class:"room-code"},snapshot.roomCode),node("a",{href:joinUrl.href,target:"_blank",rel:"noopener"},"Open player join link")):null;
  const winner = finished?snapshot.players.filter(p=>snapshot.winners.includes(p.seatId)).map(p=>p.name).join(" and "):"";
  return node("section",{class:lobby?"lobby":"board"},node("h2",{},heading),invite,
    summary(results),winner?node("p",{},`Winner: ${winner}`):null,
    !lobby&&!results?piles():null,
    snapshot.caboCaller?node("p",{},`Cabo · ${snapshot.finalTurns.length} final turns remaining`):null,
    actionStrip(),pairing?node("p",{},`Board pairing code: ${pairing}`):null,
    notice?node("p",{role:"status"},notice):null);
}
function phone() {
  const active = snapshot.phase==="playing"&&snapshot.turnSeatId===snapshot.youSeatId;
  const me = snapshot.players.find(p=>p.seatId===snapshot.youSeatId);
  const hand = [...me.cards, ...(snapshot.incoming?[snapshot.incoming]:[])];
  const selectableSlots = selectable();
  const rows = hand.length>8?Math.ceil(hand.length/8):1;
  const cols = Math.max(1,Math.ceil(hand.length/rows));
  const size = active?`${Math.min(21,38/rows)}dvh`:`${Math.min(36,57/rows)}dvh`;
  const spy = snapshot.availableActions.find(action=>action.action==="spy"||action.action==="swap");
  const targets = spy ? new Set(spy.action==="spy"?spy.slots:spy.targets):new Set();
  const opening = snapshot.availableActions.some(action=>action.action==="revealInitialCards");
  const prompt = opening?`Choose two cards · ${selection.length} of 2`:snapshot.availableActions.some(action=>action.action==="confirmInitialLook")?"Remember these two cards":status();
  const playingActions = new Set(["pairBoard","revokeBoard"]);
  const tableActions = snapshot.availableActions.some(choice=>playingActions.has(choice.action));
  const viewed = !active ? snapshot.players.filter(player=>player.seatId!==me.seatId).flatMap(player=>
    player.cards.flatMap((view,index)=>view.rank===null?[]:[`${player.name} · Card ${index+1} · ${view.rank}`])) : [];
  return node("section",{class:`phone ${active?"active":""}`},top(tableActions),
    node("div",{class:"stage"},piles(active),
      node("div",{class:"hand",style:`--cols:${cols};--size:${size}`},...hand.map((item,index)=>card(item,index,selectableSlots.has(item.slot)?()=>toggle(item.slot):null,selection.includes(item.slot)))),
      targets.size?node("div",{class:"targets"},...snapshot.players.filter(p=>p.seatId!==me.seatId).flatMap(p=>
        p.cards.map((item,index)=>targets.has(item.slot)?button(`${p.name} · Card ${index+1}`,()=>{target=item.slot;render();},target===item.slot):null))):null),
    node("footer",{},node("p",{class:"status",role:"status"},notice||prompt),
      actionStrip(choice=>!playingActions.has(choice.action)),
      ...viewed.map(label=>node("p",{class:"status"},label))),
    tableActions&&tableControlsOpen?node("aside",{class:"table-controls"},actionStrip(choice=>playingActions.has(choice.action)),
      pairing?node("p",{},`Board pairing code: ${pairing}`):null):null);
}
function animateMove(stage, previousCards, previousPiles, move) {
  if (!move || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const duration = 390;
  const easing = "ease-in-out";
  const cards = [...stage.querySelectorAll(".hand .card")];
  for (const card of cards) {
    const source = previousCards.get(card.dataset.sprite) ||
      (move.action === "drawDeck" ? previousPiles[0] : move.action === "drawDiscard" ? previousPiles[1] : null);
    if (!source) continue;
    const destination = card.getBoundingClientRect();
    card.animate([
      { transform: `translate(${source.left-destination.left}px,${source.top-destination.top}px)` },
      { transform:"translate(0,0)" },
    ], {duration,easing});
  }
  if (!["callMatch","discardHandCard","discardDrawn"].includes(move.action)) return;
  const remaining = new Set(cards.map(card=>card.dataset.sprite));
  for (const [id, source] of previousCards) {
    if (remaining.has(id)) continue;
    const destination = previousPiles[1];
    const moving = node("div",{class:"card moving-card","aria-hidden":"true",
      style:`left:${source.left}px;top:${source.top}px;width:${source.width}px;height:${source.height}px`},"✦");
    document.body.append(moving);
    moving.animate([
      {transform:"translate(0,0)",opacity:1},
      {transform:`translate(${destination.left-source.left}px,${destination.top-source.top}px) scale(.7)`,opacity:0},
    ], {duration,easing}).finished.then(()=>moving.remove(),()=>moving.remove());
  }
}
let surface;
function render() {
  const usePhone = snapshot?.role==="player"&&["initialPreview","readyToStart","playing"].includes(snapshot.phase);
  if (usePhone && surface==="phone") {
    const previous = root.firstElementChild;
    const next = phone();
    const previousStage = previous.querySelector(".stage");
    const nextStage = next.querySelector(".stage");
    const previousCards = new Map([...previousStage.querySelectorAll(".hand .card")].map(card=>[card.dataset.sprite,card.getBoundingClientRect()]));
    const previousPiles = [...previousStage.querySelectorAll(".pile .card")].map(card=>card.getBoundingClientRect());
    previous.className = next.className;
    previousStage.querySelector(".piles").replaceChildren(...nextStage.querySelector(".piles").children);
    const previousHand = previousStage.querySelector(".hand");
    const nextHand = nextStage.querySelector(".hand");
    previousHand.style.cssText = nextHand.style.cssText;
    previousHand.replaceChildren(...nextHand.children);
    previousStage.querySelector(".targets")?.remove();
    if (nextStage.querySelector(".targets")) previousStage.append(nextStage.querySelector(".targets"));
    previous.querySelector("footer").replaceWith(next.querySelector("footer"));
    previous.querySelector("header").replaceWith(next.querySelector("header"));
    previous.querySelector(".table-controls")?.remove();
    if (next.querySelector(".table-controls")) previous.append(next.querySelector(".table-controls"));
    animateMove(previousStage, previousCards, previousPiles, moveAnimation);
  } else {
    root.replaceChildren(snapshot?(usePhone?phone():node("div",{},top(),lobbyOrBoard())):home());
    surface = usePhone?"phone":"other";
  }
  moveAnimation = undefined;
}
render();
poll();
setInterval(poll, 1100);

window.addEventListener("pageshow", event => { if (event.persisted) { snapshot = undefined; render(); poll(); } });
