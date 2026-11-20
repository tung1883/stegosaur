#!/usr/bin/env bash
# Regenerates all-designs.html from the three individual mockups.
# Each design is embedded in a <script type="text/template"> block; a literal </script> inside it would
# end the block early, so it is swapped for a placeholder and restored in the browser.
set -euo pipefail
cd "$(dirname "$0")"
PH='@@SCRIPT_END@@'

{
cat <<'EOF'
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stegosaur design options</title>
<style>
  :root{--bar:#16201a;--ink:#e8efd9;--dim:#8aa090;--green:#a9cf7a;--line:#33463a}
  *{box-sizing:border-box}
  html,body{height:100%;margin:0;background:#0c130f;font:14px/1.3 system-ui,"Segoe UI",sans-serif}
  .bar{height:52px;display:flex;align-items:center;gap:14px;padding:0 14px;background:var(--bar);border-bottom:1px solid var(--line);color:var(--ink)}
  .bar img{width:30px;height:30px;border-radius:50%}
  .bar b{font-size:15px;white-space:nowrap}
  .tabs{display:flex;gap:6px;margin-left:8px}
  .tabs button{display:flex;align-items:center;gap:8px;padding:7px 14px;border:1px solid var(--line);border-radius:9px;background:transparent;color:var(--dim);font:inherit;cursor:pointer}
  .tabs button kbd{background:#26382c;color:var(--ink);border-radius:4px;padding:0 6px;font:600 12px ui-monospace,Consolas,monospace}
  .tabs button:hover{color:var(--ink)}
  .tabs button.on{background:var(--green);border-color:var(--green);color:#16240f;font-weight:600}
  .tabs button.on kbd{background:#16240f;color:var(--green)}
  .desc{margin-left:auto;color:var(--dim);font-size:12.5px;text-align:right;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  #stage{height:calc(100% - 52px)}
  iframe{display:block;width:100%;height:100%;border:0;background:#fff}
  [hidden]{display:none!important}
  @media(max-width:760px){.desc{display:none}.bar b{display:none}}
</style>
</head>
<body>
<div class="bar">
  <img src="../assets/stegosaur.png" alt=""><b>Stegosaur - design options</b>
  <div class="tabs" id="tabs">
    <button data-id="a"><kbd>A</kbd>Terminal</button>
    <button data-id="b"><kbd>B</kbd>Guided</button>
    <button data-id="c"><kbd>C</kbd>Workbench</button>
    <button data-id="d"><kbd>D</kbd>Friendly Workbench</button>
  </div>
  <div class="desc" id="desc"></div>
</div>
<div id="stage"></div>
EOF

for pair in a:a-terminal b:b-guided c:c-workbench d:d-friendly; do
  id=${pair%%:*}
  file=${pair#*:}.html
  printf '<script type="text/template" id="tpl-%s">\n' "$id"
  sed "s#</script>#${PH}#g" "$file"
  printf '</script>\n'
done

cat <<EOF
<script>
  const PH='${PH}';
EOF
cat <<'EOF'
  const info={
    a:'Terminal UI (ratatui) - keyboard + mouse, fully interactive',
    b:'Guided app - step-by-step wizard, validates as you go',
    c:'Workbench - live pixel viewer, parameters, CLI mirror',
    d:'Friendly Workbench - the workbench with a soft look and animation'
  };
  const stage=document.getElementById('stage'),desc=document.getElementById('desc'),frames={};
  const buttons=[...document.querySelectorAll('#tabs button')];
  function show(id){
    if(!info[id])id='a';
    if(!frames[id]){
      const f=document.createElement('iframe');
      f.title=id;
      f.srcdoc=document.getElementById('tpl-'+id).textContent.split(PH).join('</'+'script>');
      f.addEventListener('load',()=>{if(!f.hidden)try{f.contentWindow.focus()}catch(e){}});
      stage.appendChild(f);frames[id]=f;
    }
    for(const k in frames)frames[k].hidden=(k!==id);
    try{frames[id].contentWindow.focus()}catch(e){}
    buttons.forEach(b=>b.classList.toggle('on',b.dataset.id===id));
    desc.textContent=info[id];
    try{history.replaceState(null,'','#'+id);localStorage.setItem('stegoDesign',id)}catch(e){}
  }
  buttons.forEach(b=>b.onclick=()=>show(b.dataset.id));
  addEventListener('keydown',e=>{
    if(e.ctrlKey||e.metaKey||e.altKey)return;
    const k=e.key.toLowerCase();
    if(info[k])show(k);
  });
  let start=location.hash.slice(1);
  if(!info[start]){try{start=localStorage.getItem('stegoDesign')}catch(e){}}
  show(info[start]?start:'a');
</script>
</body>
</html>
EOF
} > all-designs.html
echo "wrote all-designs.html ($(wc -c < all-designs.html) bytes)"
