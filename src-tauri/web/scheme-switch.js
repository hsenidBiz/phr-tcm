
(function () {
  var root = document.documentElement;
  var KEY = 'tcm-page-scheme';
  var store = function (v) { try { localStorage.setItem(KEY, v); } catch (e) { /* file:// */ } };
  // The remembered choice is already on <html>: scheme-restore.js puts it
  // there from <head>, before the first paint. Doing it here, at the end
  // of the page, is what made a Light page flash dark.
  var btn = document.getElementById('scheme-switch');
  var label = btn && btn.querySelector('.scheme-label');
  var paint = function () {
    var dark = root.getAttribute('data-scheme') === 'dark';
    if (label) label.textContent = dark ? 'Light' : 'Dark';
    if (btn) btn.setAttribute('aria-pressed', dark ? 'true' : 'false');
  };
  if (btn) {
    btn.addEventListener('click', function () {
      var next = root.getAttribute('data-scheme') === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-scheme', next);
      store(next);
      paint();
    });
  }
  paint();
})();
