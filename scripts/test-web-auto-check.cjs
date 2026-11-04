const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('web/app.js', 'utf8');
const functions = source.slice(source.indexOf('  function nextAutoCheck('), source.indexOf('  function schedulePoll('));

function harness() {
  const calls = [];
  let now = 100000;
  const context = {
    auth: {authenticated:true}, document: {hidden:false}, polling:false,
    data: {config:{settings:{auto_check:true,check_all:true,check_interval_secs:30},servers:[{id:'a'},{id:'b'}],current_server:'b'},states:{}},
    Date: {now:() => now},
    api: async (path, method, body) => { calls.push({path,method,body:JSON.parse(JSON.stringify(body))}); },
    refresh: async () => {}, paintRuntime: () => {}, toast: () => {},
  };
  context.state = id => context.data.states[id] || {};
  vm.createContext(context);
  vm.runInContext(`let autoChecking = false; const lastAutoCheck = new Map(); ${functions}`, context);
  return {context,calls,advance:ms => { now += ms; }};
}

(async () => {
  for (const blocked of ['hidden','logged-out','disabled','polling']) {
    const {context,calls} = harness();
    if (blocked === 'hidden') context.document.hidden = true;
    if (blocked === 'logged-out') context.auth.authenticated = false;
    if (blocked === 'disabled') context.data.config.settings.auto_check = false;
    if (blocked === 'polling') context.polling = true;
    await context.autoCheck();
    assert.equal(calls.length, 0, blocked);
  }
  {
    const {context,calls,advance} = harness();
    await context.autoCheck();
    assert.deepEqual(calls[0], {path:'/api/probe',method:'POST',body:{ids:['a','b']}});
    await context.autoCheck();
    assert.equal(calls.length, 1);
    advance(30000);
    context.document.hidden = true;
    await context.autoCheck();
    assert.equal(calls.length, 1);
    assert.equal(context.autoCheckLabel('a'), '已暂停');
    context.document.hidden = false;
    await context.autoCheck();
    assert.equal(calls.length, 2);
    context.auth.authenticated = false;
    advance(30000);
    await context.autoCheck();
    assert.equal(calls.length, 2);
  }
  {
    const {context,calls} = harness();
    context.data.config.settings.check_all = false;
    await context.autoCheck();
    assert.deepEqual(calls[0].body.ids, ['b']);
  }
  {
    const {context,calls,advance} = harness();
    context.data.states = {a:{busy:true},b:{checked_at:100}};
    await context.autoCheck();
    assert.equal(calls.length, 0);
    context.data.states.a = {busy:false};
    await context.autoCheck();
    assert.equal(calls.length, 0);
    advance(30000);
    await context.autoCheck();
    assert.equal(calls.length, 1);
  }
  {
    const {context,calls,advance} = harness();
    let finish;
    context.api = async () => { calls.push('pending'); await new Promise(resolve => { finish = resolve; }); };
    const pending = context.autoCheck();
    advance(30000);
    await context.autoCheck();
    assert.equal(calls.length, 1);
    finish();
    await pending;
  }
  {
    const {context,calls,advance} = harness();
    context.api = async () => { calls.push('failed'); throw new Error('offline'); };
    await context.autoCheck();
    await context.autoCheck();
    assert.equal(calls.length, 1);
    advance(30000);
    await context.autoCheck();
    assert.equal(calls.length, 2);
  }
  console.log('Web auto-check: visibility, login, interval, scope, busy tasks, overlapping requests and retry passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
