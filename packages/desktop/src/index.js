
class Interpreter {
  constructor(root) {
    this.root = root;
    this.stack = [root];
    this.listeners = {
      "onclick": {}
    };
    this.lastNodeWasText = false;
    this.nodes = [root, root, root, root];
  }

  top() {
    return this.stack[this.stack.length - 1];
  }

  pop() {
    return this.stack.pop();
  }

  PushRoot(edit) {
    const id = edit.id;
    const node = this.nodes[id];
    console.log("pushing root ", node, "with id", id);
    this.stack.push(node);
  }

  PopRoot(_edit) {
    this.stack.pop();
  }

  AppendChildren(edit) {
    let root = this.stack[this.stack.length - (1 + edit.many)];

    let to_add = this.stack.splice(this.stack.length - edit.many);

    for (let i = 0; i < edit.many; i++) {
      root.appendChild(to_add[i]);
    }
  }

  ReplaceWith(edit) {
    console.log(edit);
    let root = this.nodes[edit.root];
    let els = this.stack.splice(this.stack.length - edit.m);

    console.log(root);
    console.log(els);


    root.replaceWith(...els);
  }

  Remove(edit) {
    let node = this.nodes[edit.element_id];
    node.remove();
  }

  CreateTextNode(edit) {
    const node = document.createTextNode(edit.text);
    this.nodes[edit.id] = node;
    this.stack.push(node);
  }

  CreateElement(edit) {
    const tagName = edit.tag;
    const el = document.createElement(tagName);
    this.nodes[edit.id] = el;
    this.stack.push(el);
  }

  CreateElementNs(edit) {
    let el = document.createElementNS(edit.ns, edit.tag);
    this.stack.push(el);
    this.nodes[edit.id] = el;
  }

  CreatePlaceholder(edit) {
    let el = document.createElement("pre");
    // let el = document.createComment("vroot");
    this.stack.push(el);
    this.nodes[edit.id] = el;
  }

  RemoveEventListener(edit) { }

  SetText(edit) {
    this.top().textContent = edit.text;
  }

  SetAttribute(edit) {
    const name = edit.field;
    const value = edit.value;
    const ns = edit.ns;
    const node = this.top(this.stack);
    if (ns == "style") {
      node.style[name] = value;
    } else if (ns !== undefined) {
      node.setAttributeNS(ns, name, value);
    } else {
      node.setAttribute(name, value);
    }
    if (name === "value") {
      node.value = value;
    }
    if (name === "checked") {
      node.checked = true;
    }
    if (name === "selected") {
      node.selected = true;
    }
  }
  RemoveAttribute(edit) {
    const name = edit.field;
    const node = this.top(this.stack);
    node.removeAttribute(name);

    if (name === "value") {
      node.value = null;
    }
    if (name === "checked") {
      node.checked = false;
    }
    if (name === "selected") {
      node.selected = false;
    }
  }

  InsertAfter(edit) {
    let old = this.nodes[edit.element_id];
    let new_nodes = this.stack.splice(edit.many);
    old.after(...new_nodes);
  }

  InsertBefore(edit) {
    let old = this.nodes[edit.element_id];
    let new_nodes = this.stack.splice(edit.many);
    old.before(...new_nodes);
  }

  NewEventListener(edit) {
    const event_name = edit.event_name;
    const mounted_node_id = edit.mounted_node_id;
    const scope = edit.scope;

    const element = this.top();
    element.setAttribute(`dioxus-event-${event_name}`, `${scope}.${mounted_node_id}`);

    console.log("listener map is", this.listeners);
    if (this.listeners[event_name] === undefined) {
      console.log("adding listener!");
      this.listeners[event_name] = "bla";
      this.root.addEventListener(event_name, (event) => {
        const target = event.target;
        const val = target.getAttribute(`dioxus-event-${event_name}`);
        const fields = val.split(".");
        const scope_id = parseInt(fields[0]);
        const real_id = parseInt(fields[1]);

        console.log(`parsed event with scope_id ${scope_id} and real_id ${real_id}`);

        rpc.call('user_event', {
          event: event_name,
          scope: scope_id,
          mounted_dom_id: real_id,
        }).then((reply) => {
          console.log(reply);
          this.stack.push(this.root);

          let edits = reply.edits;

          for (let x = 0; x < edits.length; x++) {
            let edit = edits[x];
            console.log(edit);

            let f = this[edit.type];
            f.call(this, edit);
          }

          console.log("initiated");
        }).catch((err) => {
          console.log("failed to initiate", err);
        });
      });
    }
  }
}

async function initialize() {
  const reply = await rpc.call('initiate');
  let root = window.document.getElementById("_dioxusroot");
  const interpreter = new Interpreter(root);
  console.log(reply);

  let pre_rendered = reply.pre_rendered;
  if (pre_rendered !== undefined) {
    root.innerHTML = pre_rendered;
  }

  const edits = reply.edits;

  apply_edits(edits, interpreter);
}

function apply_edits(edits, interpreter) {
  for (let x = 0; x < edits.length; x++) {
    let edit = edits[x];
    console.log(edit);
    let f = interpreter[edit.type];
    f.call(interpreter, edit);
  }

  console.log("stack completed: ", interpreter.stack);
}

initialize();
