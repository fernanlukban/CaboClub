const root = document.querySelector("#club");
const connection = document.querySelector("#connection");
const query = new URLSearchParams(location.search);
const use3d = query.get("scene") === "3d";
const sceneMotion = matchMedia("(prefers-reduced-motion: reduce)");
let sceneFailed = false;
let sceneLayout;
let sceneAnchors = [];

function applySceneLayout() {
  if (!sceneLayout || sceneLayout[0] !== snapshot?.roomId || sceneLayout[1] !== snapshot?.revision) return;
  const anchors = new Map(sceneLayout[2].map(anchor => [anchor.key, anchor]));
  for (const [element, key] of sceneAnchors) {
    const anchor = anchors.get(key);
    if (!anchor) continue;
    const position = `${anchor.x},${anchor.y},${anchor.width},${anchor.height},${anchor.angle}`;
    if (element.dataset.scenePosition === position) continue;
    element.dataset.scenePosition = position;
    for (const property of ["x", "y", "width", "height", "angle"]) {
      element.style.setProperty(`--scene-${property}`, `${anchor[property]}${property === "angle" ? "deg" : "px"}`);
    }
  }
}

function bindSceneAnchors() {
  if (!document.body.classList.contains("bevy-scene")) return;
  sceneAnchors = [...root.querySelectorAll("[data-scene-anchor], .card[data-sprite]")].map(element => {
    let key = element.dataset.sceneAnchor;
    if (!key) {
      const pile = element.closest("[data-pile]");
      key = pile ? `pile:${pile.dataset.pile}` :
        `${element.closest(".match-reveal") ? "match" : "hand"}:${element.dataset.sprite}`;
    }
    return [element, key];
  });
  applySceneLayout();
}


function syncScene() {
  if (!use3d || sceneFailed || !window.wasmBindings?.set_scene_frame) return;
  const accepted = window.wasmBindings.set_scene_frame(JSON.stringify({
    snapshot: snapshot ?? null, busy, reducedMotion: sceneMotion.matches,
    optimistic: pendingMove?.data ?? null,
  }));
  if (accepted !== undefined) selection = JSON.parse(accepted);
  document.body.classList.toggle("bevy-scene", accepted !== undefined && !!snapshot && snapshot.phase !== "lobby");
}

const isIPad = /iPad/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
function readSession(key) {
  try { return JSON.parse(localStorage.getItem(key) || "null"); }
  catch { return null; }
}
const room = query.get("room")?.toUpperCase();
const previousBoard = isIPad && room && readSession("cabo-session-board")?.room === room;
const tabletopPath = "/tabletop";
const onTabletopPath = [tabletopPath, tabletopPath + "/"].includes(location.pathname);
const mode = onTabletopPath || query.has("board") || (isIPad && !query.has("play") && (!room || previousBoard)) ? "board" : "player";
if (mode === "board" && (location.pathname !== tabletopPath || query.has("board") || query.has("play"))) {
  const url = new URL(location.href);
  url.pathname = tabletopPath;
  url.searchParams.delete("board");
  url.searchParams.delete("play");
  history.replaceState(null, "", url);
}
const key = `cabo-session-${mode}`;
let session = readSession(key);
if (room && session?.room !== room) session = null;
let snapshot;
let selection = [];
let starterSeat;
let testRoomsEnabled = false;
let dismissedSwap;
let busy = false;
let polling = false;
let pendingMove;
let page = query.has("room") ? "join" : "home";
let notice = "";
let pairing = "";
let insertLeft = false;
let tableControlsOpen = false;
let moveAnimation;
let animateMatchFlip = false;
let privateFlipSlot;
let animateTabletopSwap = false;

const labels = {
  dealRound:"Deal cards",dealNextRound:"Deal next round",dealNewGame:"New game",
  cancelSetup:"Cancel deal",revealInitialCards:"Reveal 2",confirmInitialLook:"I’ve looked",
  beginPlay:"Start game",drawDeck:"Draw from deck",drawDiscard:"Take discard",
  discardDrawn:"Discard drawn card",discardHandCard:"Discard selected card",callMatch:"Call match",
  callCabo:"Call Cabo",useDrawnPower:"Use card power",peek:"Peek",
  confirmSpy:"Confirm Spy",confirmSwap:"Confirm Swap",acknowledgeSpy:"Done",
  restartTest:"Restart test",restartTestNewSeed:"Restart with new seed",
  pairBoard:"Pair a board",revokeBoard:"Revoke board access",skipTurn:"Skip turn",
};
const errors = {
  staleRevision:"The table changed. Choose your move again.",staleDeal:"This deal has changed.",
  invalidName:"Enter a name with 1 to 22 characters.",roomUnavailable:"This room is full or already playing.",
  invalidPairing:"That board pairing code has expired or is incorrect.",roomNotFound:"This room has expired or does not exist.",
  invalidSession:"Your session ended. Join again to continue.",actionUnavailable:"That move is unavailable now.",
  seatTaken:"That seat is taken. Choose another seat.",
  staleTest:"This test was already restarted.",
  stalePower:"That power has changed. Check the current selection.",
  invalidSeed:"Enter a whole-number seed between 0 and 18446744073709551615.",
  testRoomsDisabled:"Test rooms are unavailable here.",
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
async function request(url, payload, credentials = session, signal) {
  const headers = {};
  if (payload) headers["Content-Type"] = "application/json";
  if (credentials) headers.Authorization = `Bearer ${credentials.token}`;
  const response = await fetch(url, {
    method:payload?"POST":"GET", headers, body:payload?JSON.stringify(payload):undefined, cache:"no-store", signal,
  });
  const data = await response.json();
  return [response, data];
}
function accept(next, force = false, renderNow = true) {
  if (!session || next.roomCode !== session.room) return;
  if (next.role !== mode) {
    leave("That session belongs to a different device mode. Join again to continue.");
    return;
  }
  if (!force && snapshot && next.roomId === snapshot.roomId && next.revision <= snapshot.revision) return;
  if (snapshot && snapshot.dealId !== next.dealId) { selection = []; starterSeat = undefined; }
  const allowed = selectable(next);
  selection = selection.filter(slot => allowed.has(slot));
  animateTabletopSwap = mode==="board" && snapshot?.pendingPower?.phase==="swapReview"
    && !next.pendingPower && snapshot.dealId===next.dealId
    && !next.message.endsWith("’s turn was skipped.");
  animateMatchFlip = Boolean(next.matchFlip && (next.roomId !== snapshot?.roomId
    || next.matchFlip.revision !== snapshot?.matchFlip?.revision));
  if (next.matchOutcome?.revision === next.revision) {
    notice = next.message;
    if (!force && snapshot?.matchFlip && snapshot.dealId === next.dealId)
      moveAnimation = { action:"callMatch", match:next.matchOutcome };
  } else if (snapshot?.matchOutcome && notice === snapshot.message) {
    notice = "";
  }
  const look = next.privatePowerResult;
  privateFlipSlot = snapshot?.roomId===next.roomId && look?.slot != null
    && look.resultId !== snapshot.privatePowerResult?.resultId ? look.slot : undefined;
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
    if (data.snapshot.role !== mode) { notice = "Could not join in this device mode."; return; }
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
  if (!session || (busy && !pendingMove) || polling) return;
  polling = true;
  const active = session;
  try {
    const [response, data] = await request(endpoint(), undefined, active);
    if (session !== active || (busy && !pendingMove)) return;
    if (response.ok) {
      connection.hidden = !pendingMove?.checking;
      accept(data);
    } else if ([401,404].includes(response.status)) {
      leave(errors[data.error]);
    }
  } catch {
    if (session === active && !pendingMove) { connection.textContent = "Reconnecting…"; connection.hidden = false; }
  } finally { polling = false; }
}
async function reconcileMove(move) {
  let delay = 300;
  while (session === move.session) {
    const abort = new AbortController();
    move.abort = abort;
    const deadline = setTimeout(() => abort.abort(), 5000);
    try {
      const result = await request(move.url, move.data, move.session, abort.signal);
      const [response, body] = result;
      if (body?.outcome === "accepted" || body?.outcome === "rejected" ||
          [401,404].includes(response.status)) return result;
    } catch {
      // A timeout or lost response leaves the command outcome unknown.
    } finally {
      clearTimeout(deadline);
    }
    if (session !== move.session) return;
    await new Promise(resolve => setTimeout(resolve, delay));
    delay = Math.min(delay * 2, 3000);
  }
}

async function submit(choice, explicitSlots) {
  if (!choice || busy || !snapshot) return;
  const active = session;
  const slots = explicitSlots ?? (choice.max ? selection.slice() : []);
  if (slots.length < choice.min || slots.length > choice.max || slots.includes(undefined)) return;
  const commandId = globalThis.crypto?.randomUUID?.() || Date.now().toString(36) + Math.random().toString(36);
  let data;
  if (use3d && !sceneFailed && window.wasmBindings?.build_scene_command) {
    const command = window.wasmBindings.build_scene_command(choice.action, JSON.stringify(explicitSlots ?? null), commandId, insertLeft);
    if (!command) return;
    data = JSON.parse(command);
  } else {
    data = {
      commandId, expectedRevision:snapshot.revision, action:choice.action, slots, insertLeft, dealId:snapshot.dealId,
      powerId:snapshot.pendingPower?.powerId ?? null,
      testGeneration:snapshot.test?.generation ?? null,
    };
  }
  busy = true;
  const url = endpoint("/commands");
  const move = use3d && !sceneFailed && window.wasmBindings?.build_scene_command
    ? {data, url, session:active, checking:false} : null;
  if (move) {
    pendingMove = move;
    move.checkTimer = setTimeout(() => {
      if (pendingMove !== move) return;
      move.checking = true;
      connection.textContent = "Checking your move…";
      connection.hidden = false;
    }, 700);
  }
  render();
  try {
    let result;
    if (move) result = await reconcileMove(move);
    else {
      try { result = await request(url, data, active); }
      catch { result = await request(url, data, active); }
    }
    if (!result || session !== active) return;
    const [response, body] = result;
    if ([401,404].includes(response.status)) { leave(errors[body.error]); return; }
    if (response.ok) {
      notice = "";
      if (choice.action === "pairBoard") { pairing = body.detail; tableControlsOpen = true; }
      const nextRevision = body.snapshot?.revision === data.expectedRevision + 1;
      if (nextRevision && ["drawDeck","drawDiscard","discardHandCard","discardDrawn"].includes(choice.action)) {
        moveAnimation = { action:choice.action };
      }
      if (choice.action === "skipTurn" && nextRevision) notice = body.detail;
      if (choice.action === "revokeBoard") pairing = "";
      selection = [];
      if (use3d) window.wasmBindings?.clear_scene_selection?.();
    } else notice = errors[body.detail] || "That action could not be applied.";
    if (body.snapshot) accept(body.snapshot, false, false);
  } catch { notice = "Connection lost. Check the table before trying again."; }
  finally {
    if (move) clearTimeout(move.checkTimer);
    if (pendingMove === move && move) {
      pendingMove = undefined;
      connection.hidden = true;
    }
    if (session === active) { busy = false; render(); }
  }
}
function leave(message = "") {
  pendingMove?.abort?.abort();
  if (pendingMove) clearTimeout(pendingMove.checkTimer);
  pendingMove = undefined;
  busy = false;
  connection.hidden = true;
  save(null); snapshot = undefined; selection = []; starterSeat = undefined; pairing = ""; tableControlsOpen = false; notice = message;
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
      mode==="player"?node("a",{href:tabletopPath},"Open tabletop mode"):null,
      mode==="board"&&testRoomsEnabled?button("TEST room",()=>navigate("test")):null,notice?node("p",{role:"status"},notice):null);
  }
  if (page === "test") {
    const form = node("form",{submit:event=>{
      event.preventDefault();
      const data = new FormData(event.currentTarget);
      connect("/api/rooms",{board:true,testScenario:data.get("scenario"),seed:data.get("seed").trim()});
    }},
    node("label",{},"Scenario",node("select",{name:"scenario"},
      node("option",{value:"random"},"Random play"),
      node("option",{value:"spy"},"Spy drill"),
      node("option",{value:"swap"},"Swap drill"))),
    node("label",{},"Seed (optional)",node("input",{name:"seed",inputmode:"numeric",maxlength:"20"})),
    node("button",{type:"submit",class:"primary",disabled:busy},"Create TEST room"));
    return node("section",{class:"home"},brand,node("h2",{},"TEST room"),form,
      notice?node("p",{role:"status"},notice):null,button("Back",()=>navigate("home")));
  }
  const creating = page === "create";
  const form = node("form",{submit:event=>{
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const room = String(data.get("room") ?? "").trim().toUpperCase();
    const direction = data.get("direction") || null;
    if (creating) connect("/api/rooms",{board:false,name:data.get("name"),direction});
    else if (mode === "board") connect(`/api/rooms/${encodeURIComponent(room)}/board`,{code:data.get("code")});
    else connect(`/api/rooms/${encodeURIComponent(room)}/join`,{name:data.get("name"),direction});
  }},creating?null:field("Room code","room",query.get("room") || "",8),
    mode==="board"?field("Board pairing code","code","",10):field("Your name","name"),
    mode==="player"?node("label",{},"Seat direction",node("select",{name:"direction","aria-label":"Seat direction"},
      node("option",{value:""},"Any open seat"),
      ...compassSeats().map(seat=>node("option",{value:seat.direction},seat.label)))):null,
    node("button",{type:"submit",class:"primary",disabled:busy},creating?"Create room":"Join room"));
  return node("section",{class:"home"},brand,node("h2",{},creating?"Create a game":"Join a game"),form,
    notice?node("p",{role:"status"},notice):null,button("Back",()=>navigate("home")));
}
async function openGameLog() {
  const activeSession = session;
  const feedback = node("p",{role:"status"},"Loading game log…");
  const content = node("div",{class:"game-log-content"});
  const dialog = node("dialog",{class:"game-log","aria-labelledby":"game-log-title"},
    node("header",{class:"top"},node("h2",{id:"game-log-title"},"Game log"),
      button("Close",()=>dialog.close())),
    feedback,content);
  dialog.addEventListener("close",()=>dialog.remove(),{once:true});
  document.body.append(dialog);
  dialog.showModal();
  try {
    const [response, log] = await request(endpoint("/log"),undefined,activeSession);
    if (!dialog.open || activeSession !== session) return;
    if (!response.ok) throw new Error("Log unavailable");
    const json = JSON.stringify(log,null,2);
    const copy = button("Copy game log", async () => {
      try {
        await navigator.clipboard.writeText(json);
        feedback.textContent = "Game log copied.";
      } catch {
        const field = node("textarea",{"aria-label":"Game log JSON",readonly:"",rows:5});
        field.value = json;
        content.prepend(field);
        field.focus();
        field.select();
        let copied = false;
        try { copied = document.execCommand("copy"); } catch { /* Leave the text selected for manual copy. */ }
        if (copied) field.remove();
        feedback.textContent = copied ? "Game log copied." : "Select and copy the game log JSON.";
      }
    },true);
    const entries = log.entries.map(entry=>{
      const action = entry.action?.replace(/[A-Z]/g,letter=>" "+letter.toLowerCase()) ?? "joined";
      const actor = entry.board ? "Board" : `Seat ${entry.seat}`;
      const pile = entry.after.discardTop===null ? "empty" : String(entry.after.discardTop);
      return node("li",{},`${actor} · ${action} · ${entry.result} · discard ${pile}`);
    });
    content.replaceChildren(copy,node("ol",{start:log.entries[0]?.sequence ?? 1},...entries));
    feedback.textContent = log.truncated ? "Showing the latest 1,024 events." :
      (entries.length ? "" : "No moves have been logged yet.");
  } catch {
    if (dialog.open) feedback.textContent = "Could not load the game log. Close and try again.";
  }
}

function top(showTableControls = false) {
  return node("header",{class:"top"},node("span",{class:"brand"},"Cabo Club"),
    node("span",{},`${snapshot.roomCode} · ${snapshot.round ? "Round " + snapshot.round : "Lobby"}`),
    node("div",{class:"top-controls"},snapshot.test?button("Game log",openGameLog):null,showTableControls?button("Table",()=>{tableControlsOpen=!tableControlsOpen;render();}):null,button("Leave",()=>leave())));
}
function card(view, index = 0, action, selected = false, name, lookedAt = false, privateLook = false) {
  const rank = view?.rank;
  const face = rank !== null && rank !== undefined;
  const flipping = (animateMatchFlip && snapshot.matchFlip.newlyFaceUpSlots.includes(view?.slot))
    || (privateLook && privateFlipSlot===view.slot);
  const props = mode==="board" && !action ? { role:"img" } :
    { type:"button", "aria-pressed":String(selected), disabled:!action || busy, click:action ?? (()=>{}) };
  return node(mode==="board"&&!action?"div":"button",{
    ...props,class:`card ${face?"face":""} ${selected?"selected":""} ${view?"":"empty"} ${flipping?"card-flipping":""} ${lookedAt?"opening-looked":""}`,
    "aria-label": name || `Card ${index+1}${face?", rank "+rank:", face down"}`,
    style:`--index:${index}`,
    "data-sprite":view?.slot === undefined ? (name ?? `card-${index}`) : String(view.slot),
  },node("span",{class:"card-inner","aria-hidden":"true"},view?(face?String(rank):"✦"):"—"),
    face?node("span",{class:"card-corners","aria-hidden":"true"},
      ...[0,1,2,3].map(()=>node("span",{},String(rank)))):null);

}
function piles(interactive = false) {
  const draw = snapshot.availableActions.find(x=>x.action==="drawDeck");
  const discard = snapshot.availableActions.find(x=>x.action==="drawDiscard");
  const deckCard = card(snapshot.deckCount?{rank:null}:null,0,interactive&&draw?()=>submit(draw):null,false,mode==="board"?"Face-down deck":"Draw from deck");
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
    node("div",{class:"pile","data-pile":"deck"},deckCard,
      node("span",{class:"pile-caption","data-scene-anchor":"label:deck"},`Deck · ${snapshot.deckCount}`)),
    node("div",{class:"pile","data-pile":"discard"},card(snapshot.discardTop===null?null:{rank:snapshot.discardTop},0,
      interactive&&discard?()=>submit(discard):null,false,`${mode==="board"?"Discard pile":"Take discard"}${snapshot.discardTop===null?"":", rank "+snapshot.discardTop}`),
      node("span",{class:"pile-caption","data-scene-anchor":"label:discard"},"Discard")));
}
function status() {
  const turn = snapshot.players.find(p=>p.seatId===snapshot.turnSeatId);
  if (snapshot.matchFlip) return turn?.seatId===snapshot.youSeatId
    ? "Revealing your match" : `${turn?.name} is revealing a match`;
  if (notice) return notice;
  if (snapshot.phase==="initialPreview") return "Waiting for opening looks";
  if (snapshot.phase==="readyToStart") return snapshot.availableActions.some(choice=>choice.action==="beginPlay")
    ? "Everyone is ready" : "Everyone is ready · Start on the tabletop screen";
  if (turn) return turn.seatId===snapshot.youSeatId?"Your turn":`${turn.name}’s turn`;
  return snapshot.message;
}
function toggle(slot) {
  if (document.body.classList.contains("bevy-scene")) {
    if (window.wasmBindings.toggle_scene_selection(slot)) render();
    return;
  }
  if (selection.includes(slot)) selection = selection.filter(id=>id!==slot);
  else {
    const limit = Math.max(...snapshot.availableActions.filter(action=>action.slots.includes(slot)&&action.action!=="spy").map(action=>action.max),1);
    selection = limit===1 ? [slot] : selection.length<limit ? [...selection,slot] : selection;
  }
  render();
}
function actionStrip(filter = () => true) {
  const visible = snapshot.availableActions.filter(choice=>filter(choice)
    && !["stagePowerTarget","selectPowerOwnCard","acknowledgeSpy","acknowledgePeek"].includes(choice.action));
  if (!visible.length) return null;
  return node("div",{class:"actions"},...visible.map(choice=>{
    let label = labels[choice.action];
    if (choice.action==="beginPlay" && snapshot.round>1) label="Start round";
    if (choice.action==="beginPlay" && choice.max) {
      const candidates = compassSeats(snapshot.players)
        .filter(({player})=>player && choice.slots.includes(player.seatId));
      const defaultStarter = candidates.length===1 ? candidates[0].player :
        snapshot.test ? candidates.find(({player})=>!player.bot)?.player : undefined;
      const selected = choice.slots.includes(starterSeat) ? starterSeat : defaultStarter?.seatId;
      const picker = node("select",{"aria-label":"Starting player",disabled:busy},
        node("option",{value:""},"Choose who starts"),
        ...candidates.map(seat=>node("option",{value:seat.player.seatId},seat.title)));
      picker.value = selected ?? "";
      picker.addEventListener("change",()=>{
        starterSeat = Number(picker.value) || undefined;
        render();
      });
      return node("div",{class:"actions"},picker,
        button(label,()=>submit(choice,[selected]),true,busy||!choice.slots.includes(selected)));
    }
    if (choice.action==="useDrawnPower") {
      const rank = snapshot.incoming?.rank;
      label = rank>=11?"Use Swap":rank>=9?"Use Spy":"Use Peek";
    }
    const staged = ["confirmSpy","confirmSwap"].includes(choice.action);
    const valid = staged || !choice.max || (selection.length>=choice.min && selection.length<=choice.max
      && selection.every(id=>choice.slots.includes(id)));
    if (choice.action==="skipTurn") {
      const actor = snapshot.players.find(player=>player.seatId===snapshot.turnSeatId);
      label = `Skip ${actor.name}’s turn`;
      return button(label,()=>{
        if (window.confirm(`Skip ${actor.name}’s turn? This spends their current turn.`)) submit(choice);
      },false,busy);
    }
    return button(label,()=>submit(choice,staged?choice.slots:undefined),
      choice.action!=="pairBoard"&&choice.action!=="revokeBoard",busy||!valid);
  }),visible.some(action=>action.chooseSide)&&selection.length>=2
    ?button(insertLeft?"Keep failed draw on left":"Keep failed draw on right",()=>{insertLeft=!insertLeft;render();}):null);
}
function summary(showCards = false) {
  const occupied = compassSeats(snapshot.players).filter(seat=>seat.player);
  return node("div",{class:"seats"},...occupied.map(({player:p,title,direction})=>{
    const result = snapshot.results.find(item=>item.seatId===p.seatId);
    return node("div",{class:`seat ${p.seatId===snapshot.turnSeatId?"current":""}`},
      node("div",{class:"tabletop-caption","data-scene-anchor":showCards ? `seat:${direction}` : undefined},
        node("strong",{},title),node("small",{},["initialPreview","readyToStart"].includes(snapshot.phase)?(p.ready?"Ready":"Looking"): `${p.score} points${result?` · +${result.points} this round`:""}`)),
      showCards?node("div",{class:"result-cards"},...p.cards.map((item,index)=>card(item,index))):null);
  }));
}
const compassDirections = ["North", "East", "South", "West"];

function compassSeats(players = []) {
  return compassDirections.map(label=>{
    const direction = label.toLowerCase();
    const player = players.find(seat=>seat.direction===direction);
    return { direction, label, player, title: `${label} · ${player?.name ?? "Open"}` };
  });
}

function powerPrompt() {
  const pending = snapshot.pendingPower;
  if (!pending) return null;
  const actor = snapshot.players.find(player=>player.seatId===pending.actorSeatId)?.name;
  const mine = snapshot.youSeatId===pending.actorSeatId;
  switch (pending.phase) {
    case "swapSelectingOwn": return mine?"Choose a card to give":`Waiting for ${actor} to choose their card`;
    case "spySelecting":
    case "swapSelectingOpponent": return mine?"Choose an opponent card":`${actor} is choosing an opponent card`;
    case "spyReview":
    case "swapReview": return mine?"Review the selected cards":`Waiting for ${actor} to confirm`;
    case "peekSelecting": return mine?"Choose one of your cards":`${actor} is choosing a card`;
    case "peekViewing":
    case "spyViewing": return mine?"Finish your private look":`${actor} is finishing their turn`;
    default: return null;
  }
}

function powerToast() {
  const result = snapshot.privatePowerResult;
  if (!result) return null;
  if (result.kind==="spy" || result.kind==="peek") {
    const isPeek = result.kind==="peek";
    const done = snapshot.availableActions.find(choice=>choice.action===(isPeek?"acknowledgePeek":"acknowledgeSpy"));
    return node("aside",{class:"power-toast",role:"status"},
      isPeek ? "Peek · Private look" : `Spy · ${result.targetName}’s card`,
      node("span",{class:"visually-hidden"},`Rank ${result.rank}`),
      done?button("Done",()=>submit(done),true,busy):null);
  }
  const receipt = `${snapshot.roomId}:${result.resultId}`;
  if (dismissedSwap===receipt || sessionStorage.getItem("cabo-dismissed-swap")===receipt) return null;
  return node("aside",{class:"power-toast",role:"status"},"Swap complete",
    button("Dismiss",()=>{
      dismissedSwap = receipt;
      sessionStorage.setItem("cabo-dismissed-swap",receipt);
      render();
    }));
}

function tabletop(center, miniature = false) {
  const targetChoice = snapshot.availableActions.find(choice=>choice.action==="stagePowerTarget");
  const peekChoice = miniature && snapshot.availableActions.find(choice=>choice.action==="peek");
  const ownChoice = miniature && snapshot.availableActions.find(choice=>choice.action==="selectPowerOwnCard");
  const look = miniature ? snapshot.privatePowerResult : null;
  const activePlayer = snapshot.players.find(player=>player.seatId===snapshot.turnSeatId);
  return node("div",{class:`tabletop ${miniature?"mini-tabletop":""}`},...compassSeats(snapshot.players).map(({direction,label,player,title})=>{
    const current = player && player.seatId===snapshot.turnSeatId;
    const hand = current && snapshot.incoming ? [...player.cards, snapshot.incoming] : player?.cards;
    const result = snapshot.results.find(item=>item.seatId===player?.seatId);
    const roundStatus = powerPrompt() || (current ? "Your turn" : activePlayer ? `${activePlayer.name}’s turn` :
      snapshot.phase==="initialPreview" ? (player?.ready ? "Ready · waiting for others" : "Opening look") :
      snapshot.phase==="readyToStart" ? "Ready to start" :
      snapshot.phase==="roundResults" ? "Round results" :
      snapshot.phase==="gameOver" ? "Game over" : "Lobby");
    return node("section",{class:`tabletop-seat ${direction} ${current?"current":""}`,"aria-label":`${label} seat`},
      node("div",{class:"tabletop-seat-content"},
        hand?.length?node("div",{class:"tabletop-hand",style:`--cards:${hand.length}`},
          ...hand.map((item,index)=>{
            const action = [targetChoice,ownChoice,peekChoice].find(choice=>choice?.slots?.includes(item.slot));
            const pick = action ? (action.action==="peek" ? ()=>toggle(item.slot) : ()=>submit(action,[item.slot])) : null;
            const selected = snapshot.pendingPower?.targetSlot===item.slot
              || (miniature && (snapshot.pendingPower?.ownSlot===item.slot || selection.includes(item.slot)));
            const lookedAt = player.openingLookSlots?.includes(item.slot);
            const privateLook = look?.slot===item.slot;
            const visibleCard = privateLook ? { ...item, rank:look.rank } : item;
            const label = `${player.name} · Card ${index+1}${visibleCard.rank===null?", face down":", rank "+visibleCard.rank}${lookedAt?", looked at during opening":""}`;
            return card(visibleCard,index,pick,selected||privateLook,label,lookedAt,privateLook && item.rank===null);
          })):null,
        node("div",{class:"tabletop-caption","data-scene-anchor":`seat:${direction}`},
          node("strong",{},title),
          player&&!miniature?node("small",{},`${roundStatus} · ${player.score} points${result?` · +${result.points} this round`:""}${snapshot.nextStarterSeatId===player.seatId?" · Starts next":""}`):null)));
  }),node("div",{class:"tabletop-center"},center));
}

function lobbyOrBoard() {
  const lobby = snapshot.phase==="lobby";
  const finished = snapshot.phase==="gameOver";
  const results = snapshot.phase==="roundResults" || finished;
  const heading = lobby?"Your table":finished?"Game over":results?"Round results":
    snapshot.phase==="readyToStart"?"Ready to play":snapshot.phase==="initialPreview"?"Opening look":status();
  const joinUrl = new URL("/",location.origin);
  joinUrl.searchParams.set("play", "");
  joinUrl.searchParams.set("room",snapshot.roomCode);
  if (use3d) joinUrl.searchParams.set("scene","3d");
  const invite = lobby?node("div",{class:"invite"},
    node("img",{src:`/api/qr?url=${encodeURIComponent(joinUrl.href)}`,alt:"Scan to join the Cabo table"}),
    node("span",{class:"room-code"},snapshot.roomCode),mode==="player"?node("a",{href:joinUrl.href,target:"_blank",rel:"noopener"},"Open player join link"):null):null;
  const winner = finished?snapshot.players.filter(p=>snapshot.winners.includes(p.seatId)).map(p=>p.name).join(" and "):"";
  if (mode==="board") {
    return node("section",{class:"board tabletop-view"},node("h2",{},heading),
      tabletop(lobby?invite:piles()),
      node("div",{class:"tabletop-hud"},snapshot.test?node("p",{class:"status"},`TEST room · ${snapshot.test.scenario} · Seed ${snapshot.test.seed}`):null,
      powerPrompt()?node("p",{class:"status",role:"status"},powerPrompt()):null,
      snapshot.message.endsWith("’s turn was skipped.")?node("p",{class:"status",role:"status"},snapshot.message):null,
      winner?node("p",{},`Winner: ${winner}`):null,
      snapshot.caboCaller?node("p",{},`Cabo · ${snapshot.finalTurns.length} final turns remaining`):null,
      snapshot.availableActions.length?actionStrip():null,
      notice?node("p",{role:"status"},notice):null));
  }
  return node("section",{class:lobby?"lobby":results?"board result-view":"board"},node("h2",{},heading),invite,
    summary(results),!lobby&&!results?piles():null,
    node("div",{class:"tabletop-hud"},winner?node("p",{},`Winner: ${winner}`):null,
      snapshot.caboCaller?node("p",{},`Cabo · ${snapshot.finalTurns.length} final turns remaining`):null,
      actionStrip(),powerToast(),pairing?node("p",{},`Board pairing code: ${pairing}`):null,
      notice?node("p",{role:"status"},notice):null));
}
function phone() {
  const active = snapshot.phase==="playing"&&snapshot.turnSeatId===snapshot.youSeatId;
  const me = snapshot.players.find(p=>p.seatId===snapshot.youSeatId);
  const closingLook = pendingMove?.data.action === "confirmInitialLook" &&
    pendingMove.data.dealId === snapshot.dealId && snapshot.phase === "initialPreview";
  const hand = [...me.cards.map(card => closingLook ? {...card, rank:null} : card),
    ...(active && snapshot.incoming ? [snapshot.incoming] : [])];
  const selectableSlots = selectable();
  const rows = hand.length>8?Math.ceil(hand.length/8):1;
  const cols = Math.max(1,Math.ceil(hand.length/rows));
  const size = active?`${Math.min(21,38/rows)}dvh`:`${Math.min(36,57/rows)}dvh`;
  const ownChoice = snapshot.availableActions.find(action=>action.action==="selectPowerOwnCard");
  const mini = active && ["peekSelecting","peekViewing","spySelecting","spyReview","spyViewing",
    "swapSelectingOpponent","swapReview"].includes(snapshot.pendingPower?.phase);
  const privateResult = powerToast();
  const opening = snapshot.availableActions.some(action=>action.action==="revealInitialCards");
  const prompt = closingLook ? "Ready" : opening?`Choose two cards · ${selection.length} of 2`:snapshot.availableActions.some(action=>action.action==="confirmInitialLook")?"Remember these two cards":powerPrompt() || status();
  const playingActions = new Set(["pairBoard","revokeBoard","skipTurn"]);
  const tableActions = snapshot.availableActions.some(choice=>playingActions.has(choice.action));
  const flip = snapshot.matchFlip;
  const revealing = flip && flip.actorSeatId !== me.seatId
    ? snapshot.players.find(player=>player.seatId===flip.actorSeatId) : null;
  const revealedCards = revealing?.cards.filter(item=>flip.slots.includes(item.slot));
  return node("section",{class:`phone ${active?"active":""}`},top(tableActions),
    node("div",{class:`stage ${mini?"has-mini-tabletop":""}`},piles(active),
      revealedCards?.length ? node("div",{class:"match-reveal","aria-label":`${revealing.name}’s match cards`},
        ...revealedCards.map(item=>card(item,revealing.cards.indexOf(item)))) : null,
      node("div",{class:"hand",style:`--cols:${cols};--size:${size}`},...hand.map((item,index)=>card(item,index,ownChoice?.slots.includes(item.slot)?()=>submit(ownChoice,[item.slot]):
        (!snapshot.pendingPower || snapshot.pendingPower.phase==="peekSelecting")&&selectableSlots.has(item.slot)?()=>toggle(item.slot):null,
        snapshot.pendingPower?.ownSlot===item.slot||selection.includes(item.slot)))),
      mini?tabletop(piles(),true):null),
    node("footer",{},node("p",{class:"status",role:"status"},notice||prompt),
      !notice&&snapshot.message.endsWith("’s turn was skipped.")?node("p",{role:"status"},snapshot.message):null,
      actionStrip(choice=>!playingActions.has(choice.action)),
      privateResult),
    tableActions&&tableControlsOpen?node("aside",{class:"table-controls"},actionStrip(choice=>playingActions.has(choice.action)),
      pairing?node("p",{},`Board pairing code: ${pairing}`):null):null);
}
function animateMove(stage, previousCards, previousPiles, move) {
  if (!move || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const duration = 390;
  const easing = "ease-in-out";
  const cards = [...stage.querySelectorAll(".hand .card")];
  for (const card of cards) {
    const slot = Number(card.dataset.sprite);
    const matchSource = move.match?.incomingSlot === slot && move.match.source === "discardTop" ? previousPiles[1] :
      move.match?.penaltySlot === slot ? previousPiles[0] : null;
    const source = previousCards.get(card.dataset.sprite) || matchSource ||
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
    if (remaining.has(id) || (move.match && !move.match.discardedSlots.includes(Number(id)))) continue;
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
  syncScene();
  const usePhone = snapshot?.role==="player"&&["initialPreview","readyToStart","playing"].includes(snapshot.phase);
  if (usePhone && surface==="phone") {
    const previous = root.firstElementChild;
    const next = phone();
    const previousStage = previous.querySelector(".stage");
    const nextStage = next.querySelector(".stage");
    const domMotion = !document.body.classList.contains("bevy-scene") && moveAnimation;
    const previousCards = domMotion ? new Map([...previousStage.querySelectorAll(".hand .card")].map(card=>[card.dataset.sprite,card.getBoundingClientRect()])) : null;
    const previousPiles = domMotion ? [...previousStage.querySelectorAll(".pile .card")].map(card=>card.getBoundingClientRect()) : null;
    previous.className = next.className;
    previousStage.querySelector(".piles").replaceChildren(...nextStage.querySelector(".piles").children);
    const previousHand = previousStage.querySelector(".hand");
    const nextHand = nextStage.querySelector(".hand");
    previousHand.style.cssText = nextHand.style.cssText;
    previousHand.replaceChildren(...nextHand.children);
    previousStage.querySelector(".match-reveal")?.remove();
    const reveal = nextStage.querySelector(".match-reveal");
    if (reveal) previousHand.before(reveal);
    previousStage.className = nextStage.className;
    previousStage.querySelector(".mini-tabletop")?.remove();
    if (nextStage.querySelector(".mini-tabletop")) previousStage.append(nextStage.querySelector(".mini-tabletop"));
    previous.querySelector("footer").replaceWith(next.querySelector("footer"));
    previous.querySelector("header").replaceWith(next.querySelector("header"));
    previous.querySelector(".table-controls")?.remove();
    if (next.querySelector(".table-controls")) previous.append(next.querySelector(".table-controls"));
    if (domMotion) animateMove(previousStage, previousCards, previousPiles, moveAnimation);
  } else {
    const previousCards = !document.body.classList.contains("bevy-scene") && animateTabletopSwap && !matchMedia("(prefers-reduced-motion: reduce)").matches
      ? new Map([...root.querySelectorAll(".tabletop-hand .card")].map(card=>[card.dataset.sprite,card.getBoundingClientRect()]))
      : null;
    root.replaceChildren(snapshot?(usePhone?phone():node("div",{},top(),lobbyOrBoard())):home());
    surface = usePhone?"phone":"other";
    if (previousCards) {
      for (const card of root.querySelectorAll(".tabletop-hand .card")) {
        const previous = previousCards.get(card.dataset.sprite);
        if (!previous) continue;
        const current = card.getBoundingClientRect();
        const x = previous.left - current.left;
        const y = previous.top - current.top;
        if (Math.hypot(x,y)>8) card.animate([
          {transform:`translate(${x}px,${y}px)`}, {transform:"translate(0,0)"},
        ],{duration:390,easing:"ease-in-out"});
      }
    }
  }
  animateTabletopSwap = false;
  moveAnimation = undefined;
  animateMatchFlip = false;
  bindSceneAnchors();
}
if (use3d) {
  window.addEventListener("TrunkApplicationStarted", () => { syncScene(); bindSceneAnchors(); });
  window.addEventListener("cabo-scene-layout", event => {
    sceneLayout = JSON.parse(event.detail);
    applySceneLayout();
  });
  document.querySelector("#table").addEventListener("webglcontextlost", () => {
    sceneFailed = true;
    document.body.classList.remove("bevy-scene");
    render();
  });
  sceneMotion.addEventListener("change", syncScene);
  window.addEventListener("cabo-scene-pick", event => {
    if (busy || !document.body.classList.contains("bevy-scene")) return;
    const key = event.detail;
    if (key === "selection") { render(); return; }
    const [action, slot] = key === "deck" ? ["drawDeck"] :
      key === "discard" ? ["drawDiscard"] : key.split(":");
    const choice = snapshot.availableActions.find(choice => choice.action === action);
    if (choice) submit(choice, slot === undefined ? undefined : [Number(slot)]);
  });
  let pointerStart;
  window.addEventListener("pointerdown", event => {
    pointerStart = undefined;
    if (!document.body.classList.contains("bevy-scene") || !event.isPrimary || event.button!==0 ||
        event.target.closest("button, input, select, a, dialog")) return;
    pointerStart = { id:event.pointerId, x:event.clientX, y:event.clientY, revision:snapshot?.revision };
  });
  window.addEventListener("pointerup", event => {
    const start = pointerStart;
    pointerStart = undefined;
    if (!start || start.id!==event.pointerId || start.revision!==snapshot?.revision) return;
    window.wasmBindings.pick_scene_card(start.x, start.y);
  });
  window.addEventListener("pointercancel", () => { pointerStart = undefined; });
}
render();
poll();
setInterval(poll, 1100);

window.addEventListener("pageshow", event => { if (event.persisted) { snapshot = undefined; render(); poll(); } });

if (mode==="board") {
  request("/api/capabilities",undefined,null).then(([response, data])=>{
    if (response.ok && data.includes("testRooms")) {
      testRoomsEnabled = true;
      if (!session && page==="home") render();
    }
  }).catch(()=>{});
}
