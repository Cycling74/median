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
          "text": "2. MSP inlet order \u2014 two signal inlets, then an integer inlet, then a proxy inlet."
        }
      },
      {
        "box": {
          "id": "start",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            65.0,
            900.0,
            40.0
          ],
          "text": "Click ezdac~ to enable DSP, then click the sampling toggle. No signal is connected to the speakers. Stop both toggles when finished."
        }
      },
      {
        "box": {
          "id": "sig0",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            40.0,
            145.0,
            130.0,
            22.0
          ],
          "text": "sig~ 0.125",
          "outlettype": [
            "signal"
          ]
        }
      },
      {
        "box": {
          "id": "sig1",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            240.0,
            145.0,
            130.0,
            22.0
          ],
          "text": "sig~ 0.75",
          "outlettype": [
            "signal"
          ]
        }
      },
      {
        "box": {
          "id": "integer",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            440.0,
            145.0,
            70.0,
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
          "id": "proxy",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            640.0,
            145.0,
            100.0,
            22.0
          ],
          "text": "probe",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "probe",
          "maxclass": "newobj",
          "numinlets": 4,
          "numoutlets": 2,
          "patching_rect": [
            40.0,
            245.0,
            720.0,
            22.0
          ],
          "text": "median.validate~",
          "outlettype": [
            "signal",
            "signal"
          ]
        }
      },
      {
        "box": {
          "id": "snap0",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            40.0,
            365.0,
            130.0,
            22.0
          ],
          "text": "snapshot~",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "snap1",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            500.0,
            365.0,
            130.0,
            22.0
          ],
          "text": "snapshot~",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "left",
          "maxclass": "flonum",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            40.0,
            425.0,
            130.0,
            22.0
          ],
          "outlettype": [
            "float",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "right",
          "maxclass": "flonum",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            500.0,
            425.0,
            130.0,
            22.0
          ],
          "outlettype": [
            "float",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "sample",
          "maxclass": "toggle",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            800.0,
            245.0,
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
            800.0,
            310.0,
            130.0,
            22.0
          ],
          "text": "metro 100",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "dsp",
          "maxclass": "ezdac~",
          "numinlets": 2,
          "numoutlets": 0,
          "patching_rect": [
            800.0,
            145.0,
            45.0,
            45.0
          ]
        }
      },
      {
        "box": {
          "id": "expected",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            505.0,
            900.0,
            65.0
          ],
          "text": "Expected: left display 0.125; right display 0.75. Integer message: signals: integer inlet 2 value 17. Proxy message: signals: proxy inlet 3. Hover all four inlets and both outlets to check assistance."
        }
      },
      {
        "box": {
          "id": "repeat",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25.0,
            610.0,
            900.0,
            65.0
          ],
          "text": "Repeat message checks with DSP off/on, Overdrive off/on, and Scheduler in Audio Interrupt off/on. Record settings. Save/reopen the patch and repeat creation/deletion."
        }
      }
    ],
    "lines": [
      {
        "patchline": {
          "source": [
            "sig0",
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
            "sig1",
            0
          ],
          "destination": [
            "probe",
            1
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "integer",
            0
          ],
          "destination": [
            "probe",
            2
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "proxy",
            0
          ],
          "destination": [
            "probe",
            3
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "sample",
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
            "probe",
            0
          ],
          "destination": [
            "snap0",
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
            "snap0",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "snap0",
            0
          ],
          "destination": [
            "left",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "probe",
            1
          ],
          "destination": [
            "snap1",
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
            "snap1",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "snap1",
            0
          ],
          "destination": [
            "right",
            0
          ]
        }
      }
    ]
  }
}
