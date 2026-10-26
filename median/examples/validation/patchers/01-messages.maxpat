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
      1000.0,
      760.0
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
            20.0,
            900.0,
            40.0
          ],
          "text": "1. Messages and threads \u2014 open the Max console before opening/reopening this patch."
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
            65.0,
            900.0,
            40.0
          ],
          "text": "Install median-validation (including its init folder), restart Max, then reopen. Expected on load: messages: loadbang; main=true scheduler=false audio=false."
        }
      },
      {
        "box": {
          "id": "int",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            130.0,
            65.0,
            22.0
          ],
          "text": "17",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "float",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            110.0,
            130.0,
            65.0,
            22.0
          ],
          "text": "17.5",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "direct",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            205.0,
            130.0,
            180.0,
            22.0
          ],
          "text": "direct 42 hello",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "low",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            405.0,
            130.0,
            180.0,
            22.0
          ],
          "text": "low 42 hello",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "front",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            605.0,
            130.0,
            180.0,
            22.0
          ],
          "text": "front 42 hello",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "probe",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            280.0,
            245.0,
            300.0,
            22.0
          ],
          "text": "median.validate messages",
          "outlettype": []
        }
      },
      {
        "box": {
          "id": "numbers",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            300.0,
            900.0,
            40.0
          ],
          "text": "17 is rejected: no int-to-float fallback was observed in Max. 17.5 must report float 17.5. low/front preserve 42 and hello; deferred callbacks must report main=true."
        }
      },
      {
        "box": {
          "id": "toggle",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25.0,
            390.0,
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
          "id": "metro",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            75.0,
            390.0,
            100.0,
            22.0
          ],
          "text": "metro 500",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "counter",
          "maxclass": "newobj",
          "numinlets": 5,
          "numoutlets": 4,
          "patching_rect": [
            205.0,
            390.0,
            85.0,
            22.0
          ],
          "text": "counter",
          "outlettype": [
            "int",
            "",
            "",
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "trigger",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 3,
          "patching_rect": [
            320.0,
            390.0,
            100.0,
            22.0
          ],
          "text": "t i i i",
          "outlettype": [
            "int",
            "int",
            "int"
          ]
        }
      },
      {
        "box": {
          "id": "pd",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            480.0,
            375.0,
            150.0,
            22.0
          ],
          "text": "prepend direct",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "pl",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            480.0,
            415.0,
            150.0,
            22.0
          ],
          "text": "prepend low",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "pf",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            480.0,
            455.0,
            150.0,
            22.0
          ],
          "text": "prepend front",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "scheduler",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            525.0,
            900.0,
            55.0
          ],
          "text": "With DSP off: Overdrive on gives direct main=false, scheduler=true; low/front main=true. Overdrive off gives direct/front main=true AND scheduler=true; low main=true, scheduler=false. Stop the metro afterward."
        }
      },
      {
        "box": {
          "id": "lifetime",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            600.0,
            900.0,
            55.0
          ],
          "text": "For a teardown smoke test: leave the metro running and delete/recreate median.validate. There must be no crash or later callback for a deleted instance. This does not prove the documented clock shutdown races absent."
        }
      },
      {
        "box": {
          "id": "limits",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            680.0,
            900.0,
            40.0
          ],
          "text": "This patch does not create a nested modal event loop or establish audio-thread qelem safety; those remain separate host investigations."
        }
      }
    ],
    "lines": [
      {
        "patchline": {
          "source": [
            "int",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "float",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "direct",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "low",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "front",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "toggle",
            0
          ],
          "destination": [
            "metro",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "metro",
            0
          ],
          "destination": [
            "counter",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "counter",
            0
          ],
          "destination": [
            "trigger",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "trigger",
            2
          ],
          "destination": [
            "pd",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "pd",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "trigger",
            1
          ],
          "destination": [
            "pl",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "pl",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "trigger",
            0
          ],
          "destination": [
            "pf",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "pf",
            0
          ],
          "destination": [
            "probe",
            0
          ]
        }
      }
    ]
  }
}
