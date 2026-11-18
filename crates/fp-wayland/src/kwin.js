// Frostpane window geometry reporter for KWin (5 and 6 scripting API).
// Loaded at runtime through org.kde.kwin.Scripting; reports every top-level
// window's frame geometry (logical coordinates, top of the stack first) and
// each screen's usable area to the Frostpane process over D-Bus.
var SVC = "%SERVICE%";

function list() {
    if (workspace.stackingOrder !== undefined) return workspace.stackingOrder;
    return workspace.clientList();
}

function geo(w) {
    var g = w.frameGeometry !== undefined ? w.frameGeometry : w.geometry;
    return [g.x, g.y, g.width, g.height];
}

function onDesk(w) {
    if (w.onAllDesktops) return true;
    var cur = workspace.currentDesktop;
    if (w.desktops !== undefined) {
        for (var i = 0; i < w.desktops.length; i++) {
            var d = w.desktops[i];
            if (d === cur || (d && cur && d.id !== undefined && d.id === cur.id)) return true;
        }
        return false;
    }
    return w.desktop === cur || w.desktop === -1;
}

function report() {
    var out = [];
    var ws = list();
    for (var i = ws.length - 1; i >= 0; i--) {
        var w = ws[i];
        if (!w || !(w.normalWindow || w.dialog)) continue;
        out.push({
            id: String(w.internalId),
            g: geo(w),
            min: !!w.minimized,
            desk: onDesk(w),
            above: !!w.keepAbove,
            full: !!w.fullScreen,
            op: w.opacity,
            pid: w.pid,
            cls: String(w.resourceClass),
            act: !!w.active
        });
    }
    var areas = [];
    try {
        var screens = workspace.screens !== undefined ? workspace.screens : [];
        for (var j = 0; j < screens.length; j++) {
            var s = screens[j];
            var a = workspace.clientArea(KWin.MaximizeArea, s, workspace.currentDesktop);
            var g = s.geometry;
            areas.push({ name: String(s.name), g: [g.x, g.y, g.width, g.height], a: [a.x, a.y, a.width, a.height] });
        }
    } catch (e) {}
    callDBus(SVC, "/geometry", "org.frostpane.Geometry", "Report", JSON.stringify({ windows: out, areas: areas }));
}

function on(sig) {
    if (sig && sig.connect) sig.connect(report);
}

function watch(w) {
    if (!w) return;
    on(w.frameGeometryChanged);
    on(w.geometryChanged);
    on(w.minimizedChanged);
    on(w.fullScreenChanged);
    on(w.desktopsChanged);
    on(w.desktopChanged);
    on(w.opacityChanged);
}

var all = list();
for (var k = 0; k < all.length; k++) watch(all[k]);
if (workspace.windowAdded) {
    workspace.windowAdded.connect(function (w) { watch(w); report(); });
} else if (workspace.clientAdded) {
    workspace.clientAdded.connect(function (w) { watch(w); report(); });
}
on(workspace.windowRemoved);
on(workspace.clientRemoved);
on(workspace.windowActivated);
on(workspace.clientActivated);
on(workspace.currentDesktopChanged);
on(workspace.stackingOrderChanged);
on(workspace.screensChanged);
report();
