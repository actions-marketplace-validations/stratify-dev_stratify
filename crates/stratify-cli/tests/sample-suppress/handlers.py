# stratify-ignore: dead_code
@route("/")
@login_required
def decorated_unused_handler():
    return "dead"
