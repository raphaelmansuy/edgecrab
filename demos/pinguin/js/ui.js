// UI and audio glue for Pinguin Slide Party
export const ui = {
  overlay: document.getElementById('overlay'),
  finishScreen: document.getElementById('finish-screen'),
  startBtn: document.getElementById('start-btn'),
  restartBtn: document.getElementById('restart-btn'),
  fishEl: document.querySelector('#fish-counter .value'),
  timeEl: document.querySelector('#timer .value'),
  scoreEl: document.getElementById('score-value'),
  comboEl: document.getElementById('combo-value'),
  comboFill: document.getElementById('combo-fill'),
  comboPanel: document.getElementById('combo-panel'),
  statFish: document.getElementById('stat-fish'),
  statTime: document.getElementById('stat-time'),
  statBonus: document.getElementById('stat-bonus'),
  statCombo: document.getElementById('stat-combo'),
  statScore: document.getElementById('stat-score'),
  finishTitle: document.getElementById('finish-title'),
  finishTagline: document.getElementById('finish-tagline'),
  starsEl: document.getElementById('stars'),
  qualitySelect: document.getElementById('quality-select'),
  soundBtn: document.getElementById('sound-btn'),
  aria: document.getElementById('aria-live'),
  hintText: document.getElementById('hint-text'),
  powerToast: document.getElementById('powerup-toast'),
  pickupToast: document.getElementById('pickup-toast'),
  compassArrow: document.getElementById('compass-arrow'),
  compassLabel: document.getElementById('compass-label'),
};

export const prefs = {
  quality: localStorage.getItem('pinguin-quality') || 'high',
  sound: localStorage.getItem('pinguin-sound') !== 'off',
};

export function applyQuality(renderer, _label, quality) {
  const dprMap = {
    low: 1,
    medium: Math.min(window.devicePixelRatio || 1, 1.5),
    high: Math.min(window.devicePixelRatio || 1, 2),
  };
  renderer.setPixelRatio(dprMap[quality] ?? dprMap.medium);
  renderer.shadowMap.enabled = quality !== 'low';
}

export function updateSoundButton(btn, isOn) {
  if (!btn) return;
  btn.textContent = isOn ? '🔊 Sound' : '🔇 Sound';
  btn.classList.toggle('active', isOn);
  btn.setAttribute('aria-pressed', isOn ? 'true' : 'false');
}

export function initSettings({ renderer, restart }) {
  if (ui.qualitySelect) {
    ui.qualitySelect.value = prefs.quality;
    ui.qualitySelect.addEventListener('change', (e) => {
      const quality = e.target.value;
      localStorage.setItem('pinguin-quality', quality);
      prefs.quality = quality;
      applyQuality(renderer, null, quality);
    });
  }

  if (ui.soundBtn) {
    updateSoundButton(ui.soundBtn, prefs.sound);
    ui.soundBtn.addEventListener('click', () => {
      prefs.sound = !prefs.sound;
      localStorage.setItem('pinguin-sound', prefs.sound ? 'on' : 'off');
      updateSoundButton(ui.soundBtn, prefs.sound);
      announce(prefs.sound ? 'Sound on' : 'Sound muted');
    });
  }

  ui.startBtn?.addEventListener('click', restart);
  ui.restartBtn?.addEventListener('click', restart);
}

export function announce(text) {
  if (!ui.aria) return;
  ui.aria.textContent = text;
  setTimeout(() => {
    if (ui.aria) ui.aria.textContent = '';
  }, 600);
}

export function formatTime(seconds) {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60).toString().padStart(2, '0');
  const sec = Math.floor(s % 60).toString().padStart(2, '0');
  return `${m}:${sec}`;
}

export function showToast(el, text, ms = 1200) {
  if (!el) return;
  el.textContent = text;
  el.hidden = false;
  el.classList.add('show');
  clearTimeout(el._hideTimer);
  el._hideTimer = setTimeout(() => {
    el.classList.remove('show');
    setTimeout(() => {
      el.hidden = true;
    }, 220);
  }, ms);
}

export function flashCombo() {
  document.body.classList.remove('flash-combo');
  // force reflow so animation restarts
  void document.body.offsetWidth;
  document.body.classList.add('flash-combo');
  setTimeout(() => document.body.classList.remove('flash-combo'), 400);
}

/** WebAudio SFX + soft movement hum */
export class AudioEngine {
  constructor() {
    this.ctx = null;
    this.hum = null;
    this.humGain = null;
    this.master = null;
  }

  ensure() {
    if (!prefs.sound) return false;
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return false;
      this.ctx = new AC();
      this.master = this.ctx.createGain();
      this.master.gain.value = 0.7;
      this.master.connect(this.ctx.destination);
    }
    if (this.ctx.state === 'suspended') this.ctx.resume();
    return true;
  }

  resume() {
    if (!this.ensure()) return;
    if (!this.hum) {
      this.hum = this.ctx.createOscillator();
      this.hum.type = 'sine';
      this.humGain = this.ctx.createGain();
      this.humGain.gain.value = 0;
      this.hum.connect(this.humGain).connect(this.master);
      this.hum.start();
    }
  }

  update(speed) {
    if (!prefs.sound || !this.ctx || !this.hum) return;
    const norm = Math.min(Math.abs(speed) / 22, 1);
    const freq = 70 + norm * 140 + Math.sin(performance.now() * 0.015) * 3;
    const t = this.ctx.currentTime;
    this.hum.frequency.setTargetAtTime(freq, t, 0.05);
    this.humGain.gain.setTargetAtTime(norm * 0.03, t, 0.06);
  }

  beep(freq = 440, dur = 0.12, type = 'sine', gain = 0.08, slideTo = null) {
    if (!this.ensure() || !this.master) return;
    const t = this.ctx.currentTime;
    const osc = this.ctx.createOscillator();
    const g = this.ctx.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(freq, t);
    if (slideTo != null) osc.frequency.exponentialRampToValueAtTime(Math.max(20, slideTo), t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(gain, t + 0.015);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    osc.connect(g).connect(this.master);
    osc.start(t);
    osc.stop(t + dur + 0.02);
  }

  collect(combo = 1) {
    const base = 520 + Math.min(combo, 10) * 40;
    this.beep(base, 0.1, 'triangle', 0.09, base * 1.6);
    if (combo >= 3) this.beep(base * 1.5, 0.14, 'sine', 0.05, base * 2);
  }

  bonus() {
    this.beep(660, 0.08, 'square', 0.05, 880);
    setTimeout(() => this.beep(990, 0.12, 'triangle', 0.06, 1320), 70);
  }

  powerup() {
    this.beep(300, 0.08, 'sawtooth', 0.04, 600);
    setTimeout(() => this.beep(500, 0.1, 'triangle', 0.05, 900), 60);
    setTimeout(() => this.beep(800, 0.14, 'sine', 0.05, 1200), 130);
  }

  jump() {
    this.beep(180, 0.12, 'sine', 0.05, 420);
  }

  land() {
    this.beep(120, 0.08, 'triangle', 0.04);
  }

  bump() {
    this.beep(90, 0.1, 'sawtooth', 0.04, 50);
  }

  win() {
    [523, 659, 784, 1046].forEach((f, i) => {
      setTimeout(() => this.beep(f, 0.18, 'triangle', 0.07), i * 110);
    });
  }

  lose() {
    this.beep(300, 0.2, 'sawtooth', 0.05, 120);
    setTimeout(() => this.beep(180, 0.28, 'triangle', 0.05, 80), 160);
  }

  setEnabled(enabled) {
    if (!enabled && this.humGain && this.ctx) {
      this.humGain.gain.setTargetAtTime(0, this.ctx.currentTime, 0.03);
    }
  }

  stop() {
    if (this.ctx?.state === 'running') this.ctx.suspend();
  }
}
