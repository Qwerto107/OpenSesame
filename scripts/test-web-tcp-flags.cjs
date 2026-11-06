const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const source = fs.readFileSync('web/app.js', 'utf8');
const functions = source.slice(source.indexOf('  const tcpFlagNames'), source.indexOf('  function editor(id)'));
const clickStart = source.indexOf('if (target.dataset.tcpFlag || target.dataset.flagsPreset) {');
const clickEnd = source.indexOf(' else if (target.dataset.knock)', clickStart);
const click = source.slice(clickStart, clickEnd);
const buttons = ['syn','ack','fin','rst','psh','urg'].map(flag => ({
  dataset: {tcpFlag: flag}, pressed: flag === 'syn' ? 'true' : 'false',
  getAttribute() { return this.pressed; },
  setAttribute(name, value) { this.pressed = value; },
  classList: {toggle() {}},
}));
let steps = [{port:7000,protocol:'tcp'}, {port:8000,protocol:'udp'}];
const preview = {textContent:''};
const context = vm.createContext({
  dialog: {
    querySelector: () => preview,
    querySelectorAll: selector => selector.includes('aria-pressed') ? buttons.filter(b => b.pressed === 'true') : buttons,
  },
  readSteps: () => steps,
});
vm.runInContext(`${functions}\nfunction clickFlag(target) { ${click} }`, context);
const flags = () => vm.runInContext('flagsLabel(readFlags())', context);
assert.equal(flags(), 'SYN');
for (const [preset, expected] of [['SYN+ACK','SYN+ACK'], ['FIN','FIN'], ['XMAS','FIN+PSH+URG'], ['SYN','SYN']]) {
  context.target = {dataset:{flagsPreset:preset}};
  vm.runInContext('clickFlag(target)', context);
  assert.equal(flags(), expected);
}
context.target = buttons[0];
vm.runInContext('clickFlag(target)', context);
assert.equal(flags(), 'NONE');
assert.match(preview.textContent, /tcp:7000 → udp:8000 · tcpflags=none$/);
steps = [{port:9000,protocol:'udp'}];
vm.runInContext('updateSequencePreview()', context);
assert.match(preview.textContent, /udp:9000 · tcpflags=none$/);
assert.match(vm.runInContext("flagsEditor(['syn','ack'])", context), /aria-pressed="true">ACK/);
console.log('TCP flags: presets, toggles, empty selection, saved values and live sequence preview passed.');
