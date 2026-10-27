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
      940.0
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
          "text": "A1. [js gimmeback.js]: JavaScript calls obj.message(...) on the object named validate."
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
          "text": "Expected (unverified), in the console, for each message: a line from the method itself (gimme: getcount 3; main=true ...), then the script's line. count -> 3 (number). list -> [2, 5, foo]: ints and floats doubled, the symbol kept, as an array. state -> an object with label gimme and count 0 (a dictionary reply). fail -> the error \"getfail: intentional error\" in the console and no usable value (null or undefined). Record the JavaScript value, its typeof and the elements' types for each; the script prints them. The bang button sends all four."
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
            60.0
          ],
          "text": "Leak check (unverified): the toggle starts a metro 10 that sends state loop, so getquiet runs 100 times a second and each reply is a list of 4096 ints (64 KiB) that the caller should free; the method and the script post almost nothing (the script says one line per 100 calls). Turn on the js toggle, watch Max's memory in Activity Monitor for a minute and expect no growth (a leak would show within seconds); then stop it and repeat with the v8 toggle. Growth means the caller does not free an object reply, and Reply::Atoms/Reply::Dict must change."
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
            490.0,
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
            525.0,
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
            525.0,
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
            575.0,
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
            525.0,
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
            640.0,
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
            680.0,
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
            680.0,
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
            725.0,
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
            725.0,
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
            790.0,
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
            850.0,
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
            680.0,
            825.0,
            150.0
          ],
          "text": "Observed 2026-10-27 (Max 9.2): getvalues 7 prints \"values 1 2.5 foo 7\" from print dumpout and getcount 1 2 3 prints \"count 3\": the list goes out dumpout under the selector without its leading get. Leak check (unverified): turn on the toggle (metro 10 sends getvalues 7 a hundred times a second), watch Max's memory in Activity Monitor for a minute and expect no growth; turn it off afterwards."
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
      }
    ]
  }
}
