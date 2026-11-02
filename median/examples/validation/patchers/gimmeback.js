// Gimmeback host check for [v8 gimmeback.js] in 05-gimmeback.maxpat.
// The [median.validate gimme] box has the scripting name "validate".
// Call its exposed methods directly; Maxobj.message() does not return these replies.
// The user verified direct calls and getstate's Dict reply in Max. Release the Dict
// with freepeer() after reading it, as in the verified state call below.
//
// Messages:
//   count 1 2 3    -> getcount: 3
//   list 1 2.5 foo -> getlist: [2, 5, "foo"]
//   state          -> getstate: Dict containing {label: "gimme", count: 0}
//   fail           -> getfail: intentional error in the Max console
//   state loop     -> getquiet: 4096 ints, summarized every 100th call
//   burst 6000     -> getquiet 6000 times, one console line when done
//   bang           -> count, list, state and fail with the arguments above
//
// For the memory check, compare Activity Monitor before/after repeated bursts.
// Each reply is 64 KiB of atoms: 6000 unreleased replies would be about 384 MB.
// Memory behavior of this direct-call path still needs a host check.

let calls = 0;

function target() {
    const obj = this.patcher.getnamed("validate");
    if (!obj) {
        post("gimmeback.js: no object named validate in this patcher\n");
    }
    return obj;
}

function describe(value) {
    if (Array.isArray(value)) {
        const parts = value.map(item => String(item) + ":" + typeof item);
        return "[" + parts.join(", ") + "] (Array, length " + value.length + ")";
    }
    return String(value) + " (" + typeof value + ")";
}

function count(...args) {
    const obj = target();
    if (obj) {
        post("getcount ->", describe(obj.getcount(...args)), "\n");
    }
}

function list(...args) {
    const obj = target();
    if (obj) {
        post("getlist ->", describe(obj.getlist(...args)), "\n");
    }
}

function state() {
    const obj = target();
    if (!obj) {
        return;
    }
    if (arguments.length > 0) {
        const reply = obj.getquiet();
        calls++;
        if (calls % 100 == 1) {
            post("getquiet loop call", calls, "-> length", reply.length, "\n");
        }
        return;
    }
    const d = obj.getstate();
    if (d) {
        try {
            post(JSON.stringify(d), "\n");
        } finally {
            d.freepeer();
        }
    } else {
        post("getstate ->", describe(d), "\n");
    }
}

function burst(n) {
    const obj = target();
    if (!obj) {
        return;
    }
    n = Math.floor(Number(n));
    if (!Number.isFinite(n) || n <= 0) {
        n = 1;
    }
    for (let i = 0; i < n; i++) {
        obj.getquiet();
    }
    post("burst done:", n, "calls\n");
}

function fail() {
    const obj = target();
    if (obj) {
        post("getfail ->", describe(obj.getfail()), "\n");
    }
}

function bang() {
    count(1, 2, 3);
    list(1, 2.5, "foo");
    state();
    fail();
}
