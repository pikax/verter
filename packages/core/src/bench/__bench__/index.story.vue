<script setup lang="ts">
import Dashboard from "./index.vue";
import { buildState } from "@judis/ui";
import { DbContact, DbContactAddress, DbContactCommunication, DbMatter } from "@judis/shared";
import { setI18n } from "vue-composable";
import enGB from "@/locales/en-GB.json";
import deepmerge from "deepmerge";

const router = useRouter();

const initState = buildState(Dashboard, () => {
  return {
    id: "contact-id",
  };
});

const phone = {
  primary: true,
  type: "personal",
  value: "111111111",
};

const email = {
  primary: true,
  type: "personal",
  value: "personalemail@test.com",
};

const website = {
  primary: true,
  type: "personal",
  value: "mywebsite.com",
};

const address = {
  administrativeArea: "Country",
  country: "Country",
  locality: "Local",
  postCode: "1111",
  primary: true,
  street: "Street",
  type: "personal",
};

function getContact() {
  return {
    about: "",
    addresses: <DbContactAddress[]>[],
    company: "",
    companyTitle: "",
    customFields: {},
    description: "",
    disabled: false,
    documents: {},
    emails: <DbContactCommunication[]>[],
    id: "contact-id",
    isCompany: false,
    name: { prefix: "", first: "Contact", middle: "", last: "One" },
    notes: {},
    phones: <DbContactCommunication[]>[],
    photoUrl: null,
    photoThumbUrl: null,
    websites: <DbContactCommunication[]>[],
  } as DbContact & { id: string };
}

function getCompany() {
  return {
    about: "",
    addresses: <DbContactAddress[]>[],
    companyName: "Company One",
    customFields: {},
    description: "",
    disabled: false,
    documents: {},
    emails: <DbContactCommunication[]>[],
    id: "company-one",
    isCompany: true,
    notes: {},
    phones: <DbContactCommunication[]>[],
    photoThumbUrl: null,
    photoUrl: null,
    websites: <DbContactCommunication[]>[],
  } as DbContact & { id: string };
}

function getMatter() {
  return {
    about: "Matter 1",
    closedAt: "2023-03-10",
    code: 1,
    contactId: "contact-id",
    customFields: {},
    disabled: false,
    id: "matter-id",
    location: "Some location",
    openAt: "2023-03-08",
    originatorUserId: "originator-user-id",
    pendingAt: "2023-03-09",
    practiceArea: "",
    reference: "ref-1234",
    relatedContacts: [{ contact: "Related contact", relationship: "Contact relationship" }],
    responsibleUserId: "user-id",
    status: "open",
  } as DbMatter & { id: string };
}

function setupApp(
  contacts: Array<DbContact & { id: string }>,
  matters?: Array<DbMatter & { id: string }>
) {
  router.currentRoute.value.params = { firmId: "firm-id" };

  const contactStore = useContactStore();
  contactStore.items.length = 0;
  contactStore.items.push(...contacts);
  contactStore.getById = () => Promise.resolve(contacts[0]);

  if (matters) {
    const matterStore = useMatterStore();
    matterStore.items.length = 0;
    matterStore.loaded = true;
    matterStore.items.push(...matters);
  }
}

function setupPlayground() {
  setupApp([getContact()]);
}

function setupCompanyContact() {
  setupApp([getCompany()]);
}

function setupContactAbout() {
  const contactWithAbout = deepmerge(getContact(), {
    about: "This is the contact's about description",
  });
  setupApp([contactWithAbout]);
}

function setupContactCompanyTitle() {
  const contactWithCompanyTitle = deepmerge(getContact(), {
    company: "company-one",
    companyTitle: "Company title",
  });

  setupApp([contactWithCompanyTitle, getCompany()]);
}

function setupContactPhone() {
  const contactWithPhone = deepmerge(getContact(), { phones: [phone] }) as DbContact & {
    id: string;
  };

  setupApp([contactWithPhone]);
}

function setupContactMultiplePhones() {
  const contactWithMultiplePhones = deepmerge(getContact(), {
    phones: [phone, phone],
  }) as DbContact & {
    id: string;
  };

  setupApp([contactWithMultiplePhones]);
}

function setupContactEmail() {
  const contactWithEmail = deepmerge(getContact(), { emails: [email] }) as DbContact & {
    id: string;
  };

  setupApp([contactWithEmail]);
}

function setupContactMultipleEmails() {
  const contactWithEmail = deepmerge(getContact(), {
    emails: [email, email, email],
  }) as DbContact & {
    id: string;
  };

  setupApp([contactWithEmail]);
}

function setupContactWebsite() {
  const contactWithWebsite = deepmerge(getContact(), { websites: [website] }) as DbContact & {
    id: string;
  };

  setupApp([contactWithWebsite]);
}

function setupContactMultipleWebsites() {
  const contactWithMultipleWebsites = deepmerge(getContact(), {
    websites: [website, website],
  }) as DbContact & {
    id: string;
  };

  setupApp([contactWithMultipleWebsites]);
}

function setupContactAddress() {
  const contactWithAddress = deepmerge(getContact(), { addresses: [address] }) as DbContact & {
    id: string;
  };

  setupApp([contactWithAddress]);
}

function setupContactMultipleAddresses() {
  const contactWithMultipleAddresses = deepmerge(getContact(), {
    addresses: [address, address],
  }) as DbContact & {
    id: string;
  };

  setupApp([contactWithMultipleAddresses]);
}

function setupContactFullInfo() {
  const contactFullInfo = deepmerge(getContact(), {
    company: "company-one",
    companyTitle: "Company title",
    phones: [phone, phone],
    emails: [email, email, email],
    websites: [website, website],
    addresses: [address, address],
  });

  setupApp([contactFullInfo, getCompany()]);
}

function setupContactClientMatter() {
  setupApp([getContact()], [getMatter()]);
}

function setupContactMultipleClientMatters() {
  const secondClientMatter = deepmerge(getMatter(), {
    about: "Matter 2",
    code: 2,
    id: "matter-two",
  }) as DbMatter & {
    id: string;
  };

  setupApp([getContact()], [getMatter(), secondClientMatter]);
}

function setupContactRelatedMatter() {
  const otherContact = deepmerge(getContact(), {
    id: "other-contact",
    name: { prefix: "", first: "Other", middle: "", last: "Contact" },
  }) as DbContact & { id: string };

  const relatedMatterOne = deepmerge(getMatter(), {
    contactId: "other-contact",
    relatedContacts: [{ contact: "contact-id", relationship: "Contact relationship" }],
  });

  setupApp([getContact(), otherContact], [relatedMatterOne]);
}

function setupContactMultipleRelatedMatters() {
  const otherContact = deepmerge(getContact(), {
    id: "other-contact",
    name: { prefix: "", first: "Other", middle: "", last: "Contact" },
  }) as DbContact & { id: string };

  const relatedMatterOne = deepmerge(getMatter(), {
    contactId: "other-contact",
    relatedContacts: [{ contact: "contact-id", relationship: "Contact relationship One" }],
  });

  const relatedMatterTwo = deepmerge(getMatter(), {
    about: "Matter 2",
    code: 2,
    id: "matter-two",
    contactId: "other-contact",
    relatedContacts: [{ contact: "contact-id", relationship: "Contact relationship Two" }],
  }) as DbMatter & {
    id: string;
  };

  setupApp([getContact(), otherContact], [relatedMatterOne, relatedMatterTwo]);
}

function setupContactFullDashboard() {
  const contactFullInfo = deepmerge(getContact(), {
    company: "company-one",
    companyTitle: "Company title",
    phones: [phone, phone],
    emails: [email, email, email],
    websites: [website, website],
    addresses: [address, address],
  });

  const otherContact = deepmerge(getContact(), {
    id: "other-contact",
    name: { prefix: "", first: "Other", middle: "", last: "Contact" },
  }) as DbContact & { id: string };

  const secondClientMatter = deepmerge(getMatter(), {
    about: "Matter 2",
    code: 2,
    id: "matter-two",
  }) as DbMatter & {
    id: string;
  };

  const relatedMatterOne = deepmerge(getMatter(), {
    contactId: "other-contact",
    relatedContacts: [{ contact: "contact-id", relationship: "Contact relationship One" }],
  });

  const relatedMatterTwo = deepmerge(getMatter(), {
    about: "Matter 2",
    code: 2,
    id: "matter-two",
    contactId: "other-contact",
    relatedContacts: [{ contact: "contact-id", relationship: "Contact relationship Two" }],
  }) as DbMatter & {
    id: string;
  };

  setupApp(
    [contactFullInfo, otherContact, getCompany()],
    [getMatter(), secondClientMatter, relatedMatterOne, relatedMatterTwo]
  );
}

function setupEnglishVariant({ app }: any) {
  setI18n(
    {
      locale: "en-GB",
      messages: {
        // @ts-expect-error not a full match
        ["en-GB"]: enGB,
      },
    },
    app
  );

  setupContactFullDashboard();
}
</script>

<template>
  <Story title="contacts/View/Tabs/Dashboard" icon="mdi:view-dashboard" icon-color="#3b82f6">
    <Variant
      title="Playground"
      icon="mdi:view-dashboard"
      icon-color="#3b82f6"
      :setup-app="setupPlayground"
      :init-state="initState"
      v-slot="{ state }"
    >
      <Suspense>
        <Dashboard v-bind="state" />
      </Suspense>
    </Variant>
    <Variant title="Contact dashboard as company" :setup-app="setupCompanyContact">
      <Suspense>
        <Dashboard id="company-one" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing about description" :setup-app="setupContactAbout">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant
      title="Contact showing company and respective title"
      :setup-app="setupContactCompanyTitle"
    >
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing phone number" :setup-app="setupContactPhone">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact with multiple phone number" :setup-app="setupContactMultiplePhones">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing email" :setup-app="setupContactEmail">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact with multiple emails" :setup-app="setupContactMultipleEmails">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing website" :setup-app="setupContactWebsite">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact with multiple websites" :setup-app="setupContactMultipleWebsites">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing address" :setup-app="setupContactAddress">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact with multiple addresses" :setup-app="setupContactMultipleAddresses">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing full information card" :setup-app="setupContactFullInfo">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing a client matter" :setup-app="setupContactClientMatter">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant
      title="Contact showing multiple client matters "
      :setup-app="setupContactMultipleClientMatters"
    >
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing a related matter" :setup-app="setupContactRelatedMatter">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant
      title="Contact showing multiple related matters"
      :setup-app="setupContactMultipleRelatedMatters"
    >
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant title="Contact showing full dashboard" :setup-app="setupContactFullDashboard">
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
    <Variant
      title="Contact showing full dashboard (English Variant)"
      :setup-app="setupEnglishVariant"
    >
      <Suspense>
        <Dashboard id="contact-id" />
      </Suspense>
    </Variant>
  </Story>
</template>
