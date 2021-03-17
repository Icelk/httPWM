!> cache dynamic
let toSend = null;

let mainStrength = document.getElementById("strength");

let from = document.getElementById("fromStrength");
let to = document.getElementById("toStrength");
let time = document.getElementById("transitionTime");
let interpolation = document.getElementById("interpolation");
let interpolationExtras = document.getElementById("interpolationExtras");

let day = document.getElementById("weekday");
let dayTime = document.getElementById("dayTime");
let dayOption = document.getElementById("optionTime");

let schedulerList = document.getElementById("schedulerList");
let schedulerKind = document.getElementById("schedulerKind");
let schedulerDate = document.getElementById("schedulerDate");
let schedulerWeekday = document.getElementById("schedulerWeekday");
let schedulerTime = document.getElementById("schedulerTime");
let schedulerName = document.getElementById("schedulerName");
let schedulerDescription = document.getElementById("schedulerDescription");

window.setInterval(() => {
    if (toSend !== null) {
        sendSet(toSend);
        toSend = null;
    }
}, 25);

window.addEventListener("unhandledrejection", (message) => {
    if (message.reason.message === "Failed to fetch") {
        message.preventDefault();
        sendNotification("Server is offline.", notificationError);
    }
});


let currentNotification = null;
const notificationInfo = "#2f5faf9f";
const notificationError = "#8c1c2e9f";
const notificationTransform = "translateX(calc(200% + 2em))";
let notificationTimeout = 5000;
function sendNotification(message, color) {
    tryClearNotification();

    let element = document.createElement("span");
    element.classList.add("notification");
    element.style.backgroundColor = color;
    element.innerHTML = message;
    element.addEventListener("click", () => tryClearNotification());

    document.body.appendChild(element);

    setTimeout(() => { element.style.transform = "none"; }, 20);

    let timeout1 = setTimeout(() => { currentNotification.element.style.transform = notificationTransform }, notificationTimeout);
    let timeout2 = setTimeout(() => { tryClearNotification() }, notificationTimeout + 500);

    currentNotification = { element: element, timeout1: timeout1, timeout2: timeout2 };
}
function tryClearNotification() {
    if (currentNotification !== null) {
        let element = currentNotification.element;

        element.style.transform = notificationTransform;
        setTimeout(() => { document.body.removeChild(element) }, 500);
        clearTimeout(currentNotification.timeout1);
        clearTimeout(currentNotification.timeout2);

        currentNotification = null;
    }
}
function responseNotification(response, name, quiet = false) {
    if (response.ok) {
        if (!quiet)
            sendNotification(`${name} succeeded!`, notificationInfo);
    } else {
        sendNotification(`${name} failed (${response.statusText})`, notificationError);
    }
}


function removeAllChildren(element) {
    while (element.children.length > 0) {
        element.removeChild(element.children[0]);
    }
}

async function sendSet(strength) {
    let response = await fetch(`/set-strength?strength=${strength}`);
    responseNotification(response, "Set strength", true);
}
// Day must exist, can be 'mon', 'tue', etc.
// Time can be null or "HH:MM:SS" format.
async function sendDayTime(day, time) {
    let response = await fetch("/set-day-time", {
        method: 'POST',
        headers: {
            'content-type': 'application/json'
        },
        redirect: 'error',
        body: JSON.stringify({ day: day, time: time })
    });
    responseNotification(response, "Set day time");
    // Takes a bit of time in backend to send message between threads...
    // (it's to damn fast)
    setTimeout(async () => await getAndApplyState(), 50);

}
function getAndSendDayTime() {
    if (dayOption.value === "some") {
        sendDayTime(day.value, dayTime.value);
    } else {
        sendDayTime(day.value, null);
    }

}
function getTransition() {
    return {
        from: Number(from.value),
        to: Number(to.value),
        time: Number(time.value),
        interpolation: interpolation.value,
        extras: [interpolationExtras.value]
    };
}
async function getAndSetTransition(action) {
    let response = await fetch(`/transition?action=${action}`, {
        method: 'POST',
        headers: {
            'content-type': 'application/json',
        },
        redirect: 'error',
        body: JSON.stringify(getTransition())
    });
    responseNotification(response, `${action} transition`);
}
function checkTransitionExtras() {
    interpolationExtras.style.display = (interpolation.value.endsWith("-extra")) ? "initial" : "none";
}
function checkDailySchedulerOption() {
    dayTime.style.display = (dayOption.value === "some") ? "initial" : "none";
}

async function getAndApplyState() {
    let response = await fetch("/get-state");
    responseNotification(response, "Update state", true);

    let json = await response.json();

    mainStrength.value = json.strength;
    for (const day in json.days) {
        const time = json.days[day];
        const element = document.getElementById(day);

        if (element !== null) {
            element.innerHTML = (time === null) ? `No time set.` : `Time set at ${time}`;
        }
    }

}

async function removeScheduler(name) {
    let response = await fetch(`/remove-scheduler?name=${name}`);
    responseNotification(response, "Removed scheduler");
    await overrideSchedulerList();
}

async function overrideSchedulerList() {
    let response = await fetch("/get-schedulers");
    responseNotification(response, "Get schedulers", true);
    let list = await response.json();

    const none = list.length == 0;

    if (none) {
        list.push({ name: "N/A", description: "N/A", kind: "none defined", next_occurrence: "N/A" });
    }

    removeAllChildren(schedulerList);

    for (let index = 0; index < list.length; index++) {
        const data = list[index];
        let tr = document.createElement("tr");

        let name = document.createElement("td");
        if (!none) {
            let remove = document.createElement("a");
            remove.innerHTML = "X";
            remove.classList.add("remove-scheduler");
            remove.addEventListener("click", (t) => removeScheduler(t.target.nextSibling.wholeText));
            name.appendChild(remove);
        }
        name.appendChild(document.createTextNode(data.name));
        let description = document.createElement("td");
        description.innerHTML = data.description;
        let kind = document.createElement("td");
        kind.innerHTML = data.kind;
        let next = document.createElement("td");
        next.innerHTML = data.next_occurrence;

        tr.appendChild(name);
        tr.appendChild(description);
        tr.appendChild(kind);
        tr.appendChild(next);
        schedulerList.appendChild(tr);
    }
}

function checkSchedulerAddExtras() {
    let { date, day } = getSchedulerExtras();

    schedulerDate.style.display = date ? "" : "none";
    schedulerWeekday.style.display = day ? "" : "none";
}
function getSchedulerExtras() {
    let kind = schedulerKind.value;
    let date = false;
    let day = false;

    if (kind === "at") {
        date = true;
    } else if (kind === "every-week") {
        day = true;
    }
    return { date: date, day: day };
}
async function getAndAddScheduler() {
    let name = schedulerName.value;
    let description = schedulerDescription.value;

    if (name === "" || description === "") {
        sendNotification("Please specify a name and description.", notificationInfo);
        return;
    }

    let kind = schedulerKind.value;
    let time = schedulerTime.value;
    let extras = [];
    let { date: send_date, day: send_day } = getSchedulerExtras();
    if (send_date) {
        extras.push(schedulerDate.value);
    }
    if (send_day) {
        extras.push(schedulerWeekday.value);
    }

    const body = { kind: kind, time: time, name: name, description: description, extras: extras, transition: getTransition() };

    console.log(body);

    let response = await fetch("/add-scheduler", {
        method: 'POST',
        headers: {
            "content-type": "application/json",
        },
        redirect: 'error',
        body: JSON.stringify(body),
    });
    responseNotification(response, "Added scheduler");
    await overrideSchedulerList();
}

async function load() {
    let state = getAndApplyState();
    let schedulers = overrideSchedulerList();

    let h2s = document.getElementsByTagName("h2");
    let loadedData = JSON.parse(localStorage.getItem("collapsed"));
    if (loadedData === null) {
        loadedData = {};
        localStorage.setItem("collapsed", JSON.stringify({}));
    }
    for (const header of h2s) {
        const target = header.getAttribute("toggle");
        if (target !== null) {
            const toggled = document.getElementById(target);
            header.addEventListener("click", () => {
                let data = JSON.parse(localStorage.getItem("collapsed"));

                if (toggled.style.maxHeight === "0px") {

                    // extra 2em if you make the screen smaller, max height should have headroom
                    toggled.style.maxHeight = `calc(${toggled.scrollHeight}px + 2em)`;

                    header.classList.add("expanded");
                    data[target] = "expanded";
                } else {

                    toggled.style.maxHeight = "0px";

                    header.classList.remove("expanded");
                    delete data[target];
                }
                localStorage.setItem("collapsed", JSON.stringify(data));
            });
            header.classList.add("collapse-host");

            if (loadedData !== null && loadedData[target] === "expanded") {
                // so the UI has time to draw, so we can get scrollHeight
                setTimeout(() => { toggled.style.maxHeight = `calc(${toggled.scrollHeight}px + 2em)`; }, 0);

                header.classList.add("expanded");
            } else {
                toggled.style.maxHeight = "0px";
            }
            toggled.classList.add("collapsible");
        }
    }

    await Promise.all([state, schedulers]);
}

load();
checkTransitionExtras();
checkDailySchedulerOption();
checkSchedulerAddExtras();
