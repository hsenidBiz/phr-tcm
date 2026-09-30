// In <head>, before anything is painted: the scheme the switch in the
// corner was last set to. The page is written in the app's scheme; a
// choice made with the switch used to be put back by the switch's own
// script at the END of the page - after the first paint - so a page the
// reader had set to Light opened dark and then went white. Guarded: a
// file:// page has no usable localStorage in some browsers, and it
// throws rather than returning null.
(function () {
  try {
    var v = localStorage.getItem('tcm-page-scheme');
    if (v === 'dark' || v === 'light') document.documentElement.setAttribute('data-scheme', v);
  } catch (e) { /* file:// */ }
})();
