{
  "patcher": {
    "fileversion": 1,
    "appversion": {
      "major": 9,
      "minor": 0,
      "revision": 0,
      "architecture": "x64",
      "modernui": 1
    },
    "classnamespace": "box",
    "rect": [
      50.0,
      50.0,
      1200.0,
      1230.0
    ],
    "default_fontsize": 13.0,
    "default_fontface": 0,
    "default_fontname": "Arial",
    "boxes": [
      {
        "box": {
          "id": "title",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            8.0,
            1100.0,
            40.0
          ],
          "text": "5. Gimmeback methods (A_GIMMEBACK) \u2014 three host checks. Open the Max console first; start with both toggles off. Everything marked expected (unverified) is an assumption for the run to settle."
        }
      },
      {
        "box": {
          "id": "setup",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            50.0,
            1100.0,
            40.0
          ],
          "text": "gimmeback.js must sit next to this patch (the package's patchers/ folder). Record what each check shows, including a result that differs from the expectation, in doc/validation-max.md using the README's recording block."
        }
      },
      {
        "box": {
          "id": "js-head",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            100.0,
            520.0,
            40.0
          ],
          "text": "A1. [js gimmeback.js]: JavaScript calls obj.message(...) on the object named validate. burst 6000 is the decisive memory check."
        }
      },
      {
        "box": {
          "id": "js-toggle",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            470.0,
            150.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "js-metro",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            505.0,
            150.0,
            70.0,
            22.0
          ],
          "text": "metro 10",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-bang",
          "maxclass": "button",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            195.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "js-count",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            65.0,
            195.0,
            100.0,
            22.0
          ],
          "text": "count 1 2 3",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-list",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            175.0,
            195.0,
            130.0,
            22.0
          ],
          "text": "list 1 2.5 foo",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-state",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            315.0,
            195.0,
            50.0,
            22.0
          ],
          "text": "state",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-fail",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            375.0,
            195.0,
            40.0,
            22.0
          ],
          "text": "fail",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-loop",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            470.0,
            195.0,
            90.0,
            22.0
          ],
          "text": "state loop",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "js-script",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            270.0,
            150.0,
            22.0
          ],
          "text": "js gimmeback.js",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-head",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            590.0,
            100.0,
            520.0,
            40.0
          ],
          "text": "A2. [v8 gimmeback.js]: the same script. If [js] reports undefined for every reply, record that and try this one."
        }
      },
      {
        "box": {
          "id": "v8-toggle",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            1035.0,
            150.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "v8-metro",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            1070.0,
            150.0,
            70.0,
            22.0
          ],
          "text": "metro 10",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-bang",
          "maxclass": "button",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            590.0,
            195.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "v8-count",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            630.0,
            195.0,
            100.0,
            22.0
          ],
          "text": "count 1 2 3",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-list",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            740.0,
            195.0,
            130.0,
            22.0
          ],
          "text": "list 1 2.5 foo",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-state",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            880.0,
            195.0,
            50.0,
            22.0
          ],
          "text": "state",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-fail",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            940.0,
            195.0,
            40.0,
            22.0
          ],
          "text": "fail",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-loop",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            1035.0,
            195.0,
            90.0,
            22.0
          ],
          "text": "state loop",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-script",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            590.0,
            270.0,
            150.0,
            22.0
          ],
          "text": "v8 gimmeback.js",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "validate",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            250.0,
            270.0,
            200.0,
            22.0
          ],
          "text": "median.validate gimme",
          "outlettype": [],
          "varname": "validate"
        }
      },
      {
        "box": {
          "id": "js-expect",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            320.0,
            1100.0,
            80.0
          ],
          "text": "Observed 2026-10-27 (third run): getcount is no longer in median.validate's message list (undocumented works). Maxobj.message(..) in [js] returns the message function object itself (\"function message() { [native code] } (function)\"), never the method's reply, although the method runs: a script cannot read a gimmeback reply through Maxobj.message (the simplejs route is not supported by median yet). The values below were never observed; for each message the console shows the method's own line (gimme: getcount 3; main=true ...) and the script's line. fail -> the error \"getfail: intentional error\" in the console. The bang button sends all four."
        }
      },
      {
        "box": {
          "id": "js-leak",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            405.0,
            1100.0,
            75.0
          ],
          "text": "Leak check. DECISIVE TEST: click the burst 6000 message box (the [js] one; the [v8] one will refuse it, which is fine). It calls getquiet 6000 times in a loop and posts \"burst done: 6000 calls\". Each reply is 4096 ints (64 KiB), so 6000 replies are 384 MB if they leak. Read Max's memory in Activity Monitor, click, read again, repeat a few times. A step of about 384 MB per click that never comes back means the reply leaks (Reply::Atoms/Reply::Dict must change); a few tens of MB that stay flat across repeated clicks is allocator slack. The toggle's metro 10 (state loop, 100 getquiet calls a second) is the weaker test: it was inconclusive on 2026-10-27 (memory crept and fell back, and the runs post to the console), so use burst."
        }
      },
      {
        "box": {
          "id": "cord-head",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            550.0,
            1100.0,
            22.0
          ],
          "text": "B. A patch-cord message to a gimmeback selector."
        }
      },
      {
        "box": {
          "id": "cord-count2",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            585.0,
            110.0,
            22.0
          ],
          "text": "getcount 1 2",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "cord-count0",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            150.0,
            585.0,
            80.0,
            22.0
          ],
          "text": "getcount",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "cord-obj",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            635.0,
            200.0,
            22.0
          ],
          "text": "median.validate cord",
          "outlettype": []
        }
      },
      {
        "box": {
          "id": "cord-expect",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            260.0,
            585.0,
            865.0,
            70.0
          ],
          "text": "Observed 2026-10-27 (Max 9.2): Max refuses the message (unknown message argument type, bad arguments for message \"getcount\", missing arguments for message \"getcount\") and the method's own line (cord: getcount 2; main=true ...) does not appear: a patch cord cannot reach an A_GIMMEBACK method, so median marks it undocumented and the patcher does not list it. Record any run that differs."
        }
      },
      {
        "box": {
          "id": "jit-head",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            700.0,
            1100.0,
            40.0
          ],
          "text": "C. Jitter: a message to a core-class gimmeback method replies out the dumpout outlet (outlet 2 of jit.median.validate; matrix outlets are 0 and 1, the custom outlets 3 and 4)."
        }
      },
      {
        "box": {
          "id": "jit-toggle",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            740.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "jit-metro",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            65.0,
            740.0,
            70.0,
            22.0
          ],
          "text": "metro 10",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "jit-values",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            785.0,
            100.0,
            22.0
          ],
          "text": "getvalues 7",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "jit-count",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            140.0,
            785.0,
            120.0,
            22.0
          ],
          "text": "getcount 1 2 3",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "jit-obj",
          "maxclass": "newobj",
          "numinlets": 5,
          "numoutlets": 5,
          "patching_rect": [
            25.0,
            850.0,
            200.0,
            22.0
          ],
          "text": "jit.median.validate",
          "outlettype": [
            "jit_matrix",
            "jit_matrix",
            "",
            "int",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "jit-print",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            100.0,
            910.0,
            150.0,
            22.0
          ],
          "text": "print dumpout",
          "outlettype": []
        }
      },
      {
        "box": {
          "id": "jit-expect",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            300.0,
            740.0,
            825.0,
            150.0
          ],
          "text": "Observed 2026-10-27 (Max 9.2): getvalues 7 prints \"values 1 2.5 foo 7\" from print dumpout and getcount 1 2 3 prints \"count 3\": the list goes out dumpout under the selector without its leading get. The toggle's metro 10 sends getvalues 7 a hundred times a second and each reply is printed to the console, so it measures the console as much as the reply (inconclusive on 2026-10-27: memory crept to about 1.2 GB and fell below 1 GB when stopped). Turn it off afterwards. The decisive test is C2 below."
        }
      },
      {
        "box": {
          "id": "js-burst",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            470.0,
            230.0,
            90.0,
            22.0
          ],
          "text": "burst 6000",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "v8-burst",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            1035.0,
            230.0,
            90.0,
            22.0
          ],
          "text": "burst 6000",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "jit2-head",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            960.0,
            1100.0,
            40.0
          ],
          "text": "C2. Jitter leak check, burst: getbig replies with 4096 ints (64 KiB) out the dumpout of a second jit.median.validate that has nothing attached, so nothing prints. The button runs uzi 6000, which sends getbig 6000 times."
        }
      },
      {
        "box": {
          "id": "jit2-bang",
          "maxclass": "button",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            1010.0,
            24.0,
            24.0
          ],
          "outlettype": [
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "jit2-uzi",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 3,
          "patching_rect": [
            25.0,
            1050.0,
            70.0,
            22.0
          ],
          "text": "uzi 6000",
          "outlettype": [
            "bang",
            "bang",
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "jit2-big",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            1095.0,
            60.0,
            22.0
          ],
          "text": "getbig",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "jit2-obj",
          "maxclass": "newobj",
          "numinlets": 5,
          "numoutlets": 5,
          "patching_rect": [
            25.0,
            1150.0,
            200.0,
            22.0
          ],
          "text": "jit.median.validate",
          "outlettype": [
            "jit_matrix",
            "jit_matrix",
            "",
            "int",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "jit2-expect",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            300.0,
            1010.0,
            825.0,
            60.0
          ],
          "text": "Expected (unverified): Max's memory a few tens of MB higher at most after a click and flat across repeated clicks. A step of about 384 MB per click (6000 replies of 64 KiB) that never comes back means the reply leaks. Read Activity Monitor before and after each click."
        }
      }
    ],
    "lines": [
      {
        "patchline": {
          "source": [
            "js-toggle",
            0
          ],
          "destination": [
            "js-metro",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-metro",
            0
          ],
          "destination": [
            "js-loop",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-bang",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-count",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-list",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-state",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-fail",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-loop",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-toggle",
            0
          ],
          "destination": [
            "v8-metro",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-metro",
            0
          ],
          "destination": [
            "v8-loop",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-bang",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-count",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-list",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-state",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-fail",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-loop",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "cord-count2",
            0
          ],
          "destination": [
            "cord-obj",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "cord-count0",
            0
          ],
          "destination": [
            "cord-obj",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit-toggle",
            0
          ],
          "destination": [
            "jit-metro",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit-metro",
            0
          ],
          "destination": [
            "jit-values",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit-values",
            0
          ],
          "destination": [
            "jit-obj",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit-count",
            0
          ],
          "destination": [
            "jit-obj",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit-obj",
            2
          ],
          "destination": [
            "jit-print",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "js-burst",
            0
          ],
          "destination": [
            "js-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "v8-burst",
            0
          ],
          "destination": [
            "v8-script",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit2-bang",
            0
          ],
          "destination": [
            "jit2-uzi",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit2-uzi",
            0
          ],
          "destination": [
            "jit2-big",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "jit2-big",
            0
          ],
          "destination": [
            "jit2-obj",
            0
          ]
        }
      }
    ]
  }
}
