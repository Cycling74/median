// Gimmeback host check (patch 05-gimmeback.maxpat). Used by both [js gimmeback.js] and
// [v8 gimmeback.js]. The patch has a [median.validate gimme] with the scripting name "validate".
//
// Each message calls one gimmeback method with obj.message(...) and posts what came back, as a
// JavaScript value and its typeof. The call is made directly with the arguments spelled out:
// obj.message.apply(..) is not a plain function call in [js] (the first run posted the message
// function itself as the "reply"). Whether Maxobj.message hands back the method's reply is one
// of the things this run establishes (a reply of undefined may mean the call does not pass it
// on, not that median is wrong).
//
// Observed on 2026-10-27 (Max 9.2 ARM64): [js] reaches the method with the right arguments;
// [v8] does not: its message() reports "bad arguments" / "missing arguments" for the selector,
// the method does not run, and the value is null. So the [v8] box is a negative control.
//
// Messages (the patch's message boxes):
//   count 1 2 3        -> getcount            expected (unverified): the number 3
//   list 1 2.5 foo     -> getlist             expected (unverified): [2, 5, "foo"]
//   state              -> getstate            expected (unverified): an object {label: "gimme", count: 0}
//   fail               -> getfail             expected (unverified): an error in the Max console, no value
//   state loop         -> getquiet, for the leak check: a 4096-atom list, posts nothing (the script
//                         posts only every 100th call)
//   bang               -> all four with the arguments above

var calls = 0;

function describe(v) {
    var t = typeof v;
    if (v === null) {
        return "null (" + t + ")";
    }
    if (v instanceof Array) {
        var parts = [];
        for (var i = 0; i < v.length; i++) {
            parts.push(String(v[i]) + ":" + typeof v[i]);
        }
        return "[" + parts.join(", ") + "] (Array, length " + v.length + ")";
    }
    if (t == "object") {
        var keys = [];
        try {
            for (var k in v) {
                keys.push(k + "=" + String(v[k]));
            }
        } catch (e) {
            keys.push("(enumeration failed: " + e + ")");
        }
        var extra = "";
        try {
            if (typeof v.getkeys == "function") {
                var names = v.getkeys();
                var items = [];
                if (names !== undefined && names !== null) {
                    names = [].concat(names);
                    for (var j = 0; j < names.length; j++) {
                        items.push(names[j] + "=" + String(v.get(names[j])));
                    }
                }
                extra = " getkeys: {" + items.join(", ") + "}";
            }
        } catch (e2) {
            extra = " (getkeys failed: " + e2 + ")";
        }
        return String(v) + " (object; keys {" + keys.join(", ") + "})" + extra;
    }
    return String(v) + " (" + t + ")";
}

// A direct call with up to three arguments (no Function.apply, see the header).
function call(obj, selector, args) {
    switch (args.length) {
        case 0:
            return obj.message(selector);
        case 1:
            return obj.message(selector, args[0]);
        case 2:
            return obj.message(selector, args[0], args[1]);
        default:
            return obj.message(selector, args[0], args[1], args[2]);
    }
}

function ask(selector, args) {
    var obj = this.patcher.getnamed("validate");
    if (!obj) {
        post("gimmeback.js: no object named validate in this patcher\n");
        return;
    }
    var r = call(obj, selector, args);
    post(selector, "->", describe(r), "\n");
    return r;
}

function count() {
    ask("getcount", arrayfromargs(arguments));
}

function list() {
    ask("getlist", arrayfromargs(arguments));
}

function state() {
    if (arguments.length > 0) {
        // leak check: 100 calls a second, so say something only now and then
        var obj = this.patcher.getnamed("validate");
        if (!obj) {
            return;
        }
        var r = obj.message("getquiet");
        calls++;
        if (calls % 100 == 1) {
            post("getquiet loop call", calls, "->", describe(r), "\n");
        }
        return;
    }
    ask("getstate", []);
}

function fail() {
    ask("getfail", []);
}

function bang() {
    count(1, 2, 3);
    list(1, 2.5, "foo");
    state();
    fail();
}
