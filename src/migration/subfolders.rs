/*
    Old Format:
    |
    |-> [date]
    |    |-> [shift_nr].json
    |    |-> ...
    |-> [date].json

    New Format:
    |
    |-> [date]
    |   |-> shifts
    |   |   |-> [shift_nr].json
    |   |   |-> ..
    |   |-> omloop
    |   |   |-> [date_int]_[omloop_nr].json
    |   |   |-> ..
    |   |-> deadheads
    |   |   |-> [date_int]_[omloop_nr].json
    |   |   |-> ..
    |-> [date].json
*/
